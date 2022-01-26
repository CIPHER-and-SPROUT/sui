// Copyright (c) Facebook, Inc. and its affiliates.
// SPDX-License-Identifier: Apache-2.0

use crate::downloader::*;
use async_trait::async_trait;
use fastx_framework::build_move_package_to_bytes;
use fastx_types::messages::Address::FastPay;
use fastx_types::{
    base_types::*, committee::Committee, error::FastPayError, fp_ensure, messages::*,
};
use futures::{future, StreamExt, TryFutureExt};
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::TypeTag;
use rand::seq::SliceRandom;

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;

#[cfg(test)]
use fastx_types::FASTX_FRAMEWORK_ADDRESS;

#[cfg(test)]
#[path = "unit_tests/client_tests.rs"]
mod client_tests;

// TODO: Make timeout duration configurable.
const AUTHORITY_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub type AsyncResult<'a, T, E> = future::BoxFuture<'a, Result<T, E>>;

#[async_trait]
pub trait AuthorityClient {
    /// Initiate a new order to a FastPay or Primary account.
    async fn handle_order(&mut self, order: Order) -> Result<OrderInfoResponse, FastPayError>;

    /// Confirm an order to a FastPay or Primary account.
    async fn handle_confirmation_order(
        &mut self,
        order: ConfirmationOrder,
    ) -> Result<OrderInfoResponse, FastPayError>;

    /// Handle Account information requests for this account.
    async fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, FastPayError>;

    /// Handle Object information requests for this account.
    async fn handle_object_info_request(
        &self,
        request: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, FastPayError>;
}

pub struct ClientState<AuthorityClient> {
    /// Our FastPay address.
    address: FastPayAddress,
    /// Our signature key.
    secret: KeyPair,
    /// Our FastPay committee.
    committee: Committee,
    /// How to talk to this committee.
    authority_clients: HashMap<AuthorityName, AuthorityClient>,
    /// Pending transfer.
    pending_transfer: Option<Order>,

    // The remaining fields are used to minimize networking, and may not always be persisted locally.
    /// Known certificates, indexed by TX digest.
    certificates: BTreeMap<TransactionDigest, CertifiedOrder>,
    /// The known objects with it's sequence number owned by the client.
    object_sequence_numbers: BTreeMap<ObjectID, SequenceNumber>,
    /// Confirmed objects with it's ref owned by the client.
    object_refs: BTreeMap<ObjectID, ObjectRef>,
    /// Certificate <-> object id linking map.
    object_certs: BTreeMap<ObjectID, Vec<TransactionDigest>>,
}

// Operations are considered successful when they successfully reach a quorum of authorities.
#[async_trait]
pub trait Client {
    /// Send object to a FastX account.
    async fn transfer_object(
        &mut self,
        object_id: ObjectID,
        gas_payment: ObjectID,
        recipient: FastPayAddress,
    ) -> Result<CertifiedOrder, anyhow::Error>;

    /// Receive object from FastX.
    async fn receive_object(&mut self, certificate: &CertifiedOrder) -> Result<(), anyhow::Error>;

    /// Send object to a FastX account.
    /// Do not confirm the transaction.
    async fn transfer_to_fastx_unsafe_unconfirmed(
        &mut self,
        recipient: FastPayAddress,
        object_id: ObjectID,
        gas_payment: ObjectID,
    ) -> Result<CertifiedOrder, anyhow::Error>;

    /// Synchronise client state with a random authorities, updates all object_ids and certificates, request only goes out to one authority.
    /// this method doesn't guarantee data correctness, client will have to handle potential byzantine authority
    async fn sync_client_state_with_random_authority(
        &mut self,
    ) -> Result<AuthorityName, anyhow::Error>;

    /// Call move functions in the module in the given package, with args supplied
    async fn move_call(
        &mut self,
        package_object_ref: ObjectRef,
        module: Identifier,
        function: Identifier,
        type_arguments: Vec<TypeTag>,
        gas_object_ref: ObjectRef,
        object_arguments: Vec<ObjectRef>,
        pure_arguments: Vec<Vec<u8>>,
        gas_budget: u64,
    ) -> Result<(CertifiedOrder, OrderEffects), anyhow::Error>;

    /// Publish Move modules
    async fn publish(
        &mut self,
        package_source_files_path: String,
        gas_object_ref: ObjectRef,
    ) -> Result<(CertifiedOrder, OrderEffects), anyhow::Error>;

    /// Get the object information
    async fn get_object_info(
        &mut self,
        object_info_req: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, anyhow::Error>;

    /// Get all object we own.
    async fn get_owned_objects(&self) -> Result<Vec<ObjectID>, anyhow::Error>;
}

impl<A> ClientState<A> {
    pub fn new(
        address: FastPayAddress,
        secret: KeyPair,
        committee: Committee,
        authority_clients: HashMap<AuthorityName, A>,
        certificates: BTreeMap<TransactionDigest, CertifiedOrder>,
        object_refs: BTreeMap<ObjectID, ObjectRef>,
    ) -> Self {
        let object_seq = object_refs
            .iter()
            .map(|(id, (_, seq, _))| (*id, *seq))
            .collect();

        Self {
            address,
            secret,
            committee,
            authority_clients,
            pending_transfer: None,
            certificates,
            object_sequence_numbers: object_seq,
            object_refs,
            object_certs: BTreeMap::new(),
        }
    }

    pub fn address(&self) -> FastPayAddress {
        self.address
    }

    pub fn next_sequence_number(
        &self,
        object_id: &ObjectID,
    ) -> Result<SequenceNumber, FastPayError> {
        if self.object_sequence_numbers.contains_key(object_id) {
            Ok(self.object_sequence_numbers[object_id])
        } else {
            Err(FastPayError::ObjectNotFound {
                object_id: *object_id,
            })
        }
    }

    pub fn object_refs(&self) -> &BTreeMap<ObjectID, ObjectRef> {
        &self.object_refs
    }

    pub fn pending_transfer(&self) -> &Option<Order> {
        &self.pending_transfer
    }

    pub fn certificates(&self, object_id: &ObjectID) -> impl Iterator<Item = &CertifiedOrder> {
        self.object_certs
            .get(object_id)
            .into_iter()
            .flat_map(|cert_digests| {
                cert_digests
                    .iter()
                    .filter_map(|digest| self.certificates.get(digest))
            })
    }

    pub fn all_certificates(&self) -> &BTreeMap<TransactionDigest, CertifiedOrder> {
        &self.certificates
    }

    pub fn insert_object(&mut self, object_ref: &ObjectRef, digest: &TransactionDigest) {
        let (object_id, seq, _) = object_ref;
        self.object_sequence_numbers.insert(*object_id, *seq);
        self.object_certs
            .entry(*object_id)
            .or_default()
            .push(*digest);
        self.object_refs.insert(*object_id, *object_ref);
    }

    pub fn remove_object(&mut self, object_id: &ObjectID) {
        self.object_sequence_numbers.remove(object_id);
        self.object_certs.remove(object_id);
        self.object_refs.remove(object_id);
    }
}

#[derive(Clone)]
struct CertificateRequester<A> {
    committee: Committee,
    authority_clients: Vec<A>,
    sender: Option<FastPayAddress>,
}

impl<A> CertificateRequester<A> {
    fn new(
        committee: Committee,
        authority_clients: Vec<A>,
        sender: Option<FastPayAddress>,
    ) -> Self {
        Self {
            committee,
            authority_clients,
            sender,
        }
    }
}

#[async_trait]
impl<A> Requester for CertificateRequester<A>
where
    A: AuthorityClient + Send + Sync + 'static + Clone,
{
    type Key = (ObjectID, SequenceNumber);
    type Value = Result<CertifiedOrder, FastPayError>;

    /// Try to find a certificate for the given sender and sequence number.
    async fn query(
        &mut self,
        (object_id, sequence_number): (ObjectID, SequenceNumber),
    ) -> Result<CertifiedOrder, FastPayError> {
        let request = ObjectInfoRequest {
            object_id,
            request_sequence_number: Some(sequence_number),
            request_received_transfers_excluding_first_nth: None,
        };
        // Sequentially try each authority in random order.
        // TODO: Improve shuffle, different authorities might different amount of stake.
        self.authority_clients.shuffle(&mut rand::thread_rng());
        for client in self.authority_clients.iter_mut() {
            let result = client.handle_object_info_request(request.clone()).await;
            if let Ok(response) = result {
                let certificate = response.requested_certificate.unwrap();
                if certificate.check(&self.committee).is_ok() {
                    let order = &certificate.order;
                    if let Some(sender) = self.sender {
                        if order.sender() == &sender && order.sequence_number() == sequence_number {
                            return Ok(certificate.clone());
                        }
                    } else {
                        return Ok(certificate.clone());
                    }
                }
            }
        }
        Err(FastPayError::ErrorWhileRequestingCertificate)
    }
}

impl<A> ClientState<A>
where
    A: AuthorityClient + Send + Sync + 'static + Clone,
{
    #[cfg(test)]
    async fn request_certificate(
        &mut self,
        sender: FastPayAddress,
        object_id: ObjectID,
        sequence_number: SequenceNumber,
    ) -> Result<CertifiedOrder, FastPayError> {
        CertificateRequester::new(
            self.committee.clone(),
            self.authority_clients.values().cloned().collect(),
            Some(sender),
        )
        .query((object_id, sequence_number))
        .await
    }

    /// Find the highest sequence number that is known to a quorum of authorities.
    /// NOTE: This is only reliable in the synchronous model, with a sufficient timeout value.
    #[cfg(test)]
    async fn get_strong_majority_sequence_number(&self, object_id: ObjectID) -> SequenceNumber {
        let request = ObjectInfoRequest {
            object_id,
            request_sequence_number: None,
            request_received_transfers_excluding_first_nth: None,
        };
        let mut authority_clients = self.authority_clients.clone();
        let numbers: futures::stream::FuturesUnordered<_> = authority_clients
            .iter_mut()
            .map(|(name, client)| {
                let fut = client.handle_object_info_request(request.clone());
                async move {
                    match fut.await {
                        Ok(info) => Some((*name, info.object.version())),
                        _ => None,
                    }
                }
            })
            .collect();
        self.committee.get_strong_majority_lower_bound(
            numbers.filter_map(|x| async move { x }).collect().await,
        )
    }

    /// Return owner address and sequence number of an object backed by a quorum of authorities.
    /// NOTE: This is only reliable in the synchronous model, with a sufficient timeout value.
    #[cfg(test)]
    async fn get_strong_majority_owner(
        &self,
        object_id: ObjectID,
    ) -> Option<(FastPayAddress, SequenceNumber)> {
        let request = ObjectInfoRequest {
            object_id,
            request_sequence_number: None,
            request_received_transfers_excluding_first_nth: None,
        };
        let authority_clients = self.authority_clients.clone();
        let numbers: futures::stream::FuturesUnordered<_> = authority_clients
            .iter()
            .map(|(name, client)| {
                let fut = client.handle_object_info_request(request.clone());
                async move {
                    match fut.await {
                        Ok(info) => Some((*name, Some((info.object.owner, info.object.version())))),
                        _ => None,
                    }
                }
            })
            .collect();
        self.committee.get_strong_majority_lower_bound(
            numbers.filter_map(|x| async move { x }).collect().await,
        )
    }

    #[cfg(test)]
    async fn get_framework_object_ref(&mut self) -> Result<ObjectRef, anyhow::Error> {
        self.get_object_info(ObjectInfoRequest {
            object_id: FASTX_FRAMEWORK_ADDRESS,
            request_sequence_number: None,
            request_received_transfers_excluding_first_nth: None,
        })
        .await
        .map(|response| response.object.to_object_reference())
    }

    /// Execute a sequence of actions in parallel for a quorum of authorities.
    async fn communicate_with_quorum<'a, V, F>(
        &'a mut self,
        execute: F,
    ) -> Result<Vec<V>, FastPayError>
    where
        F: Fn(AuthorityName, &'a mut A) -> AsyncResult<'a, V, FastPayError> + Clone,
    {
        let committee = &self.committee;
        let authority_clients = &mut self.authority_clients;
        let mut responses: futures::stream::FuturesUnordered<_> = authority_clients
            .iter_mut()
            .map(|(name, client)| {
                let execute = execute.clone();
                async move { (*name, execute(*name, client).await) }
            })
            .collect();

        let mut values = Vec::new();
        let mut value_score = 0;
        let mut error_scores = HashMap::new();
        while let Some((name, result)) = responses.next().await {
            match result {
                Ok(value) => {
                    values.push(value);
                    value_score += committee.weight(&name);
                    if value_score >= committee.quorum_threshold() {
                        // Success!
                        return Ok(values);
                    }
                }
                Err(err) => {
                    let entry = error_scores.entry(err.clone()).or_insert(0);
                    *entry += committee.weight(&name);
                    if *entry >= committee.validity_threshold() {
                        // At least one honest node returned this error.
                        // No quorum can be reached, so return early.
                        return Err(FastPayError::QuorumNotReached {
                            errors: error_scores.into_keys().collect(),
                        });
                    }
                }
            }
        }
        Err(FastPayError::QuorumNotReached {
            errors: error_scores.into_keys().collect(),
        })
    }

    /// Broadcast confirmation orders and execute handle order .
    async fn broadcast_and_handle_order(
        &mut self,
        sender: FastPayAddress,
        order: Order,
    ) -> Result<(Vec<(CertifiedOrder, OrderInfoResponse)>, CertifiedOrder), anyhow::Error> {
        let mut input_objects = Vec::new();
        for (object_id, seq, _) in &order.input_objects() {
            input_objects.push((*object_id, *seq));
        }

        for (object_id, target_sequence_number, _) in &order.input_objects() {
            let next_sequence_number = self.next_sequence_number(object_id).unwrap_or_default();
            fp_ensure!(
                target_sequence_number >= &next_sequence_number,
                FastPayError::UnexpectedSequenceNumber {
                    object_id: *object_id,
                    expected_sequence: next_sequence_number,
                    received_sequence: *target_sequence_number,
                }
                .into()
            );
        }

        let committee = self.committee.clone();
        let (responses, votes) = self
            .broadcast_and_execute(
                sender,
                order.input_objects(),
                Vec::new(),
                |name, authority| {
                    let order = order.clone();
                    let committee = committee.clone();
                    Box::pin(async move {
                        match authority.handle_order(order).await {
                            Ok(OrderInfoResponse {
                                signed_order: Some(inner_signed_order),
                                ..
                            }) => {
                                fp_ensure!(
                                    inner_signed_order.authority == name,
                                    FastPayError::ErrorWhileProcessingTransferOrder
                                );
                                inner_signed_order.check(&committee)?;
                                Ok((inner_signed_order.authority, inner_signed_order.signature))
                            }
                            Err(err) => Err(err),
                            _ => Err(FastPayError::ErrorWhileProcessingTransferOrder),
                        }
                    })
                },
            )
            .await?;
        let certificate = CertifiedOrder {
            order,
            signatures: votes,
        };
        // Certificate is valid because
        // * `communicate_with_quorum` ensured a sufficient "weight" of (non-error) answers were returned by authorities.
        // * each answer is a vote signed by the expected authority.
        Ok((responses, certificate))
    }

    /// Broadcast confirmation orders and execute provided authority action.
    /// The corresponding sequence numbers should be consecutive and increasing.
    async fn broadcast_and_execute<'a, V, F: 'a>(
        &'a mut self,
        sender: FastPayAddress,
        inputs: Vec<ObjectRef>,
        certificates_to_broadcast: Vec<CertifiedOrder>,
        action: F,
    ) -> Result<(Vec<(CertifiedOrder, OrderInfoResponse)>, Vec<V>), anyhow::Error>
    where
        F: Fn(AuthorityName, &'a mut A) -> AsyncResult<'a, V, FastPayError> + Send + Sync + Copy,
        V: Copy,
    {
        let requester = CertificateRequester::new(
            self.committee.clone(),
            self.authority_clients.values().cloned().collect(),
            Some(sender),
        );

        let known_certificates = inputs.iter().flat_map(|(object_id, seq, _)| {
            self.certificates(object_id)
                .cloned()
                .filter_map(move |cert| {
                    if cert.order.sender() == &sender {
                        Some(((*object_id, *seq), Ok(cert)))
                    } else {
                        None
                    }
                })
        });

        let (_, mut handle) = Downloader::start(requester, known_certificates);
        let result = self
            .communicate_with_quorum(|name, client| {
                let certificates_to_broadcast = certificates_to_broadcast.clone();
                let inputs = inputs.clone();
                let mut handle = handle.clone();
                Box::pin(async move {
                    // Sync certificate with authority
                    // Figure out which certificates this authority is missing.
                    let mut responses = Vec::new();
                    let mut missing_certificates = Vec::new();
                    for (object_id, target_sequence_number, _) in inputs {
                        let request = ObjectInfoRequest {
                            object_id,
                            request_sequence_number: None,
                            request_received_transfers_excluding_first_nth: None,
                        };
                        let response = client.handle_object_info_request(request).await?;

                        let current_sequence_number = response.object.version();
                        // Download each missing certificate in reverse order using the downloader.
                        let mut number = target_sequence_number.decrement();
                        while let Ok(seq) = number {
                            if seq < current_sequence_number {
                                break;
                            }
                            let certificate = handle
                                .query((object_id, seq))
                                .await
                                .map_err(|_| FastPayError::ErrorWhileRequestingCertificate)??;
                            missing_certificates.push(certificate);
                            number = seq.decrement();
                        }
                    }
                    // Send all missing confirmation orders.
                    missing_certificates.reverse();
                    missing_certificates.extend(certificates_to_broadcast.clone());
                    for certificate in missing_certificates {
                        responses.push((
                            certificate.clone(),
                            client
                                .handle_confirmation_order(ConfirmationOrder::new(certificate))
                                .await?,
                        ));
                    }
                    Ok((responses, action(name, client).await?))
                })
            })
            .await?;
        // Terminate downloader task and retrieve the content of the cache.
        handle.stop().await?;

        let action_results = result.iter().map(|(_, result)| *result).collect();

        // Assume all responses are the same, pick the first one.
        let order_response = result
            .iter()
            .map(|(response, _)| response.clone())
            .next()
            .unwrap_or_default();

        Ok((order_response, action_results))
    }

    /// Broadcast confirmation orders.
    /// The corresponding sequence numbers should be consecutive and increasing.
    async fn broadcast_confirmation_orders(
        &mut self,
        sender: FastPayAddress,
        inputs: Vec<ObjectRef>,
        certificates_to_broadcast: Vec<CertifiedOrder>,
    ) -> Result<Vec<(CertifiedOrder, OrderInfoResponse)>, anyhow::Error> {
        self.broadcast_and_execute(sender, inputs, certificates_to_broadcast, |_, _| {
            Box::pin(async { Ok(()) })
        })
        .await
        .map(|(responses, _)| responses)
    }

    /// Make sure we have all our certificates with sequence number
    /// in the range 0..self.next_sequence_number
    pub async fn download_certificates(
        &mut self,
    ) -> Result<BTreeMap<ObjectID, Vec<CertifiedOrder>>, FastPayError> {
        let mut sent_certificates: BTreeMap<ObjectID, Vec<CertifiedOrder>> = BTreeMap::new();

        for (object_id, next_sequence_number) in self.object_sequence_numbers.clone() {
            let known_sequence_numbers: BTreeSet<_> = self
                .certificates(&object_id)
                .flat_map(|cert| cert.order.input_objects())
                .filter_map(|(id, seq, _)| if id == object_id { Some(seq) } else { None })
                .collect();

            let mut requester = CertificateRequester::new(
                self.committee.clone(),
                self.authority_clients.values().cloned().collect(),
                None,
            );

            let entry = sent_certificates.entry(object_id).or_default();
            // TODO: it's inefficient to loop through sequence numbers to retrieve missing cert, rethink this logic when we change certificate storage in client.
            let mut number = SequenceNumber::from(0);
            while number < next_sequence_number {
                if !known_sequence_numbers.contains(&number) {
                    let certificate = requester.query((object_id, number)).await?;
                    entry.push(certificate);
                }
                number = number.increment();
            }
        }
        Ok(sent_certificates)
    }

    /// Transfers an object to a recipient address.
    async fn transfer(
        &mut self,
        object_id: ObjectID,
        gas_payment: ObjectID,
        recipient: Address,
    ) -> Result<CertifiedOrder, anyhow::Error> {
        let object_ref = self
            .object_refs
            .get(&object_id)
            .ok_or(FastPayError::ObjectNotFound { object_id })?;
        let gas_payment =
            self.object_refs
                .get(&gas_payment)
                .ok_or(FastPayError::ObjectNotFound {
                    object_id: gas_payment,
                })?;

        let transfer = Transfer {
            object_ref: *object_ref,
            sender: self.address,
            recipient,
            gas_payment: *gas_payment,
        };
        let order = Order::new_transfer(transfer, &self.secret);
        let (certificate, _) = self
            .execute_transaction(order, /* with_confirmation */ true)
            .await?;
        if let FastPay(address) = recipient {
            if address != self.address {
                self.remove_object(&object_id);
            }
        }

        Ok(certificate)
    }

    /// Update our view of certificates. Update the object_id and the next sequence number accordingly.
    /// NOTE: This is only useful in the eventuality of missing local data.
    /// We assume certificates to be valid and sent by us, and their sequence numbers to be unique.
    fn update_certificates(
        &mut self,
        object_id: &ObjectID,
        certificates: &[CertifiedOrder],
    ) -> Result<(), FastPayError> {
        for new_cert in certificates {
            // Try to get object's last seq number before the mutation, default to 0 for newly created object.
            let seq = new_cert
                .order
                .input_objects()
                .iter()
                .find_map(
                    |(id, seq, _)| {
                        if object_id == id {
                            Some(seq)
                        } else {
                            None
                        }
                    },
                )
                .cloned()
                .unwrap_or_default();

            let mut new_next_sequence_number = self.next_sequence_number(object_id)?;
            if seq >= new_next_sequence_number {
                new_next_sequence_number = seq.increment();
            }

            self.certificates
                .insert(new_cert.order.digest(), new_cert.clone());

            // Atomic update
            self.object_sequence_numbers
                .insert(*object_id, new_next_sequence_number);

            let certs = self.object_certs.entry(*object_id).or_default();

            if !certs.contains(&new_cert.order.digest()) {
                certs.push(new_cert.order.digest());
            }
        }
        // Sanity check
        let certificates_count = self.certificates(object_id).count();

        if certificates_count == usize::from(self.next_sequence_number(object_id)?) {
            Ok(())
        } else {
            Err(FastPayError::UnexpectedSequenceNumber {
                object_id: *object_id,
                expected_sequence: SequenceNumber::from(certificates_count as u64),
            })
        }
    }

    /// Execute (or retry) a order. Update local balance.
    async fn execute_transaction(
        &mut self,
        order: Order,
        with_confirmation: bool,
    ) -> Result<(CertifiedOrder, Option<OrderEffects>), anyhow::Error> {
        fp_ensure!(
            self.pending_transfer == None || self.pending_transfer.as_ref() == Some(&order),
            FastPayError::ConcurrentTransactionError.into()
        );
        self.pending_transfer = Some(order.clone());
        let result = self
            .broadcast_and_handle_order(self.address, order.clone())
            .await;
        // Clear `pending_transfer` and update `sent_certificates`,
        // and `next_sequence_number`. (Note that if we were using persistent
        // storage, we should ensure update atomicity in the eventuality of a crash.)
        self.pending_transfer = None;

        // order_info_response contains response from broadcasting old unconfirmed order, if any.
        let (mut order_info_responses, new_sent_certificate) = result?;
        assert_eq!(&new_sent_certificate.order, &order);

        // Confirm last transfer certificate if needed.
        if with_confirmation {
            let responses = self
                .broadcast_confirmation_orders(
                    self.address,
                    order.input_objects(),
                    vec![new_sent_certificate.clone()],
                )
                .await?;
            // Add new confirmed order info to the old ones.
            order_info_responses.extend(responses);
        } else {
            // The new cert will not be updated by order effect without confirmation, the new unconfirmed cert need to be added temporally.
            if let OrderKind::Transfer(_) = order.kind {
                // Only valid for object transfer, where input_objects = output_objects
                let new_sent_certificates = vec![new_sent_certificate.clone()];
                for (object_id, _, _) in order.input_objects() {
                    self.update_certificates(&object_id, &new_sent_certificates)?;
                }
            }
        }

        // Update local data using all order response.
        for (_, response) in order_info_responses.clone() {
            self.update_objects_from_order_info(response)?;
        }

        // Find order effect for the current order from all the returned order response.
        let order_effect = order_info_responses
            .into_iter()
            .find_map(|(cert, response)| {
                if cert == new_sent_certificate {
                    response
                        .signed_effects
                        .map(|signed_effect| signed_effect.effects)
                } else {
                    None
                }
            });

        Ok((new_sent_certificate, order_effect))
    }

    async fn download_own_object_ids(
        &self,
    ) -> Result<(AuthorityName, Vec<ObjectRef>), FastPayError> {
        let request = AccountInfoRequest {
            account: self.address,
        };
        // Sequentially try each authority in random order.
        let mut authorities: Vec<&AuthorityName> = self.authority_clients.keys().collect();
        // TODO: implement sampling according to stake distribution and using secure RNG. https://github.com/MystenLabs/fastnft/issues/128
        authorities.shuffle(&mut rand::thread_rng());
        // Authority could be byzantine, add timeout to avoid waiting forever.
        for authority_name in authorities {
            let authority = self.authority_clients.get(authority_name).unwrap();
            let result = timeout(
                AUTHORITY_REQUEST_TIMEOUT,
                authority.handle_account_info_request(request.clone()),
            )
            .map_err(|_| FastPayError::ErrorWhileRequestingInformation)
            .await?;
            if let Ok(AccountInfoResponse { object_ids, .. }) = &result {
                return Ok((*authority_name, object_ids.clone()));
            }
        }
        Err(FastPayError::ErrorWhileRequestingInformation)
    }

    fn update_objects_from_order_info(
        &mut self,
        order_info_resp: OrderInfoResponse,
    ) -> Result<(CertifiedOrder, OrderEffects), FastPayError> {
        if let Some(v) = order_info_resp.signed_effects {
            // The cert should be included in the response
            let cert = order_info_resp.certified_order.unwrap();
            let digest = cert.order.digest();
            self.certificates.insert(digest, cert.clone());

            for &(object_ref, owner) in v.effects.all_mutated() {
                let (object_id, seq, _) = object_ref;
                let old_seq = self
                    .object_sequence_numbers
                    .get(&object_id)
                    .cloned()
                    .unwrap_or_default();
                // only update if data is new
                if old_seq < seq {
                    if owner == self.address {
                        self.insert_object(&object_ref, &digest);
                    } else {
                        self.remove_object(&object_id);
                    }
                } else if old_seq == seq && owner == self.address {
                    // ObjectRef can be 1 version behind because it's only updated after confirmation.
                    self.object_refs.insert(object_id, object_ref);
                }
            }
            for (object_id, seq, _) in &v.effects.deleted {
                let old_seq = self
                    .object_sequence_numbers
                    .get(object_id)
                    .cloned()
                    .unwrap_or_default();
                if old_seq < *seq {
                    self.remove_object(object_id);
                }
            }
            Ok((cert, v.effects))
        } else {
            Err(FastPayError::ErrorWhileRequestingInformation)
        }
    }

    async fn get_object_info_execute(
        &mut self,
        object_info_req: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, anyhow::Error> {
        let votes = self
            .communicate_with_quorum(|_, client| {
                let req = object_info_req.clone();
                Box::pin(async move { client.handle_object_info_request(req).await })
            })
            .await?;

        votes
            .get(0)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No valid confirmation order votes"))
    }
}

#[async_trait]
impl<A> Client for ClientState<A>
where
    A: AuthorityClient + Send + Sync + Clone + 'static,
{
    async fn transfer_object(
        &mut self,
        object_id: ObjectID,
        gas_payment: ObjectID,
        recipient: FastPayAddress,
    ) -> Result<CertifiedOrder, anyhow::Error> {
        self.transfer(object_id, gas_payment, Address::FastPay(recipient))
            .await
    }

    async fn receive_object(&mut self, certificate: &CertifiedOrder) -> Result<(), anyhow::Error> {
        certificate.check(&self.committee)?;
        match &certificate.order.kind {
            OrderKind::Transfer(transfer) => {
                fp_ensure!(
                    transfer.recipient == Address::FastPay(self.address),
                    FastPayError::IncorrectRecipientError.into()
                );
                let responses = self
                    .broadcast_confirmation_orders(
                        transfer.sender,
                        certificate.order.input_objects(),
                        vec![certificate.clone()],
                    )
                    .await?;

                for (_, response) in responses {
                    self.update_objects_from_order_info(response)?;
                }

                let response = self
                    .get_object_info(ObjectInfoRequest {
                        object_id: *certificate.order.object_id(),
                        request_sequence_number: Some(transfer.object_ref.1),
                        request_received_transfers_excluding_first_nth: None,
                    })
                    .await?;
                self.object_refs
                    .insert(response.object.id(), response.object.to_object_reference());

                // Everything worked: update the local balance.
                if let Entry::Vacant(entry) = self.certificates.entry(certificate.order.digest()) {
                    self.object_sequence_numbers
                        .insert(transfer.object_ref.0, transfer.object_ref.1.increment());
                    self.object_certs
                        .entry(transfer.object_ref.0)
                        .or_default()
                        .push(certificate.order.digest());
                    entry.insert(certificate.clone());
                }
                Ok(())
            }
            OrderKind::Publish(_) | OrderKind::Call(_) => {
                unimplemented!("receiving (?) Move call or publish")
            }
        }
    }

    async fn transfer_to_fastx_unsafe_unconfirmed(
        &mut self,
        recipient: FastPayAddress,
        object_id: ObjectID,
        gas_payment: ObjectID,
    ) -> Result<CertifiedOrder, anyhow::Error> {
        let object_ref = *self
            .object_refs
            .get(&object_id)
            .ok_or(FastPayError::ObjectNotFound { object_id })?;
        let gas_payment =
            *self
                .object_refs
                .get(&gas_payment)
                .ok_or(FastPayError::ObjectNotFound {
                    object_id: gas_payment,
                })?;

        let transfer = Transfer {
            object_ref,
            sender: self.address,
            recipient: Address::FastPay(recipient),
            gas_payment,
        };
        let order = Order::new_transfer(transfer, &self.secret);
        let (new_certificate, _) = self
            .execute_transaction(order, /* with_confirmation */ false)
            .await?;
        Ok(new_certificate)
    }

    async fn sync_client_state_with_random_authority(
        &mut self,
    ) -> Result<AuthorityName, anyhow::Error> {
        if let Some(order) = self.pending_transfer.clone() {
            // Finish executing the previous transfer.
            self.execute_transaction(order, /* with_confirmation */ false)
                .await?;
        }
        // update object_ids.
        self.object_sequence_numbers.clear();
        self.object_refs.clear();

        let (authority_name, object_refs) = self.download_own_object_ids().await?;
        for object_ref in object_refs {
            let (object_id, sequence_number, _) = object_ref;
            self.object_sequence_numbers
                .insert(object_id, sequence_number);
            self.object_refs.insert(object_id, object_ref);
        }
        // Recover missing certificates.
        let new_certificates = self.download_certificates().await?;

        for (id, certs) in new_certificates {
            self.update_certificates(&id, &certs)?;
        }
        Ok(authority_name)
    }

    async fn move_call(
        &mut self,
        package_object_ref: ObjectRef,
        module: Identifier,
        function: Identifier,
        type_arguments: Vec<TypeTag>,
        gas_object_ref: ObjectRef,
        object_arguments: Vec<ObjectRef>,
        pure_arguments: Vec<Vec<u8>>,
        gas_budget: u64,
    ) -> Result<(CertifiedOrder, OrderEffects), anyhow::Error> {
        let move_call_order = Order::new_move_call(
            self.address,
            package_object_ref,
            module,
            function,
            type_arguments,
            gas_object_ref,
            object_arguments,
            pure_arguments,
            gas_budget,
            &self.secret,
        );
        let (cert, effect) = self.execute_transaction(move_call_order, true).await?;
        let effect = effect.ok_or(FastPayError::UnexpectedMessage)?;
        Ok((cert, effect))
    }

    async fn publish(
        &mut self,
        package_source_files_path: String,
        gas_object_ref: ObjectRef,
    ) -> Result<(CertifiedOrder, OrderEffects), anyhow::Error> {
        // Try to compile the package at the given path
        let compiled_modules = build_move_package_to_bytes(Path::new(&package_source_files_path))?;
        let move_publish_order =
            Order::new_module(self.address, gas_object_ref, compiled_modules, &self.secret);
        let (cert, effect) = self.execute_transaction(move_publish_order, true).await?;
        let effect = effect.ok_or(FastPayError::UnexpectedMessage)?;
        Ok((cert, effect))
    }

    async fn get_object_info(
        &mut self,
        object_info_req: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, anyhow::Error> {
        self.get_object_info_execute(object_info_req).await
    }

    async fn get_owned_objects(&self) -> Result<Vec<ObjectID>, anyhow::Error> {
        Ok(self.object_sequence_numbers.keys().copied().collect())
    }
}
