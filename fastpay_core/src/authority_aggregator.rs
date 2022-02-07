// Copyright (c) Facebook, Inc. and its affiliates.
// SPDX-License-Identifier: Apache-2.0

use crate::{authority_client::AuthorityAPI, downloader::*};
use async_trait::async_trait;
use fastx_types::object::Object;
use fastx_types::{
    base_types::*,
    committee::Committee,
    error::{FastPayError, FastPayResult},
    fp_ensure,
    messages::*,
};
use futures::{future, StreamExt, TryFutureExt};
use rand::seq::SliceRandom;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time::timeout;

// TODO: Make timeout duration configurable.
const AUTHORITY_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

const OBJECT_DOWNLOAD_CHANNEL_BOUND: usize = 1024;

#[cfg(test)]
#[path = "unit_tests/client_tests.rs"]
mod client_tests;

pub type AsyncResult<'a, T, E> = future::BoxFuture<'a, Result<T, E>>;

pub struct AuthorityAggregator<AuthorityAPI> {
    /// Our FastPay committee.
    pub committee: Committee,
    /// How to talk to this committee.
    authority_clients: BTreeMap<AuthorityName, AuthorityAPI>,
}

impl<AuthorityAPI> AuthorityAggregator<AuthorityAPI> {
    pub fn new(
        committee: Committee,
        authority_clients: BTreeMap<AuthorityName, AuthorityAPI>,
    ) -> Self {
        Self {
            committee,
            authority_clients,
        }
    }
}

pub enum ReduceOutput<S> {
    Continue(S),
    ContinueWithTimeout(S, Duration),
    End(S),
}

#[allow(dead_code)]
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
    A: AuthorityAPI + Send + Sync + 'static + Clone,
{
    type Key = (ObjectID, SequenceNumber);
    type Value = Result<CertifiedOrder, FastPayError>;

    /// Try to find a certificate for the given sender, object_id and sequence number.
    async fn query(
        &mut self,
        (object_id, sequence_number): (ObjectID, SequenceNumber),
    ) -> Result<CertifiedOrder, FastPayError> {
        // BUG(https://github.com/MystenLabs/fastnft/issues/290): This function assumes that requesting the parent cert of object seq+1 will give the cert of
        //        that creates the object. This is not true, as objects may be deleted and may not have a seq+1
        //        to look up.
        //
        //        The authority `handle_object_info_request` is now fixed to return the parent at seq, and not
        //        seq+1. But a lot of the client code makes the above wrong assumption, and the line above reverts
        //        query to the old (incorrect) behavious to not break tests everywhere.
        let inner_sequence_number = sequence_number.increment();

        let request = ObjectInfoRequest {
            object_id,
            request_sequence_number: Some(inner_sequence_number),
        };
        // Sequentially try each authority in random order.
        // TODO: Improve shuffle, different authorities might different amount of stake.
        self.authority_clients.shuffle(&mut rand::thread_rng());
        for client in self.authority_clients.iter_mut() {
            let result = client.handle_object_info_request(request.clone()).await;
            if let Ok(ObjectInfoResponse {
                parent_certificate: Some(certificate),
                ..
            }) = result
            {
                if certificate.check(&self.committee).is_ok() {
                    return Ok(certificate);
                }
            }
        }
        Err(FastPayError::ErrorWhileRequestingCertificate)
    }
}

impl<A> AuthorityAggregator<A>
where
    A: AuthorityAPI + Send + Sync + 'static + Clone,
{
    /// Sync a certificate and all its dependencies to a destination authority, using a
    /// source authority to get information about parent certificates.
    ///
    /// Note: Both source and destination may be byzantine, therefore one should always
    /// time limit the call to this function to avoid byzantine authorities consuming
    /// an unbounded amount of resources.
    async fn sync_authority_source_to_destination(
        &self,
        cert: ConfirmationOrder,
        source_authority: AuthorityName,
        destination_authority: AuthorityName,
    ) -> Result<(), FastPayError> {
        let source_client = self.authority_clients[&source_authority].clone();
        let destination_client = self.authority_clients[&destination_authority].clone();

        // This represents a stack of certificates that we need to register with the
        // destination authority. The stack is a LIFO queue, and therefore later insertions
        // represent certificates that earlier insertions depend on. Thus updating an
        // authority in the order we pop() the certificates from this stack should ensure
        // certificates are uploaded in causal order.
        let digest = cert.certificate.order.digest();
        let mut missing_certificates: Vec<_> = vec![cert.clone()];

        // We keep a list of certificates already processed to avoid duplicates
        let mut candidate_certificates: HashSet<TransactionDigest> =
            vec![digest].into_iter().collect();
        let mut attempted_certificates: HashSet<TransactionDigest> = HashSet::new();

        while let Some(target_cert) = missing_certificates.pop() {
            match destination_client
                .handle_confirmation_order(target_cert.clone())
                .await
            {
                Ok(_) => continue,
                Err(FastPayError::LockErrors { .. }) => {}
                Err(e) => return Err(e),
            }

            // If we are here it means that the destination authority is missing
            // the previous certificates, so we need to read them from the source
            // authority.

            // The first time we cannot find the cert from the destination authority
            // we try to get its dependencies. But the second time we have already tried
            // to update its dependencies, so we should just admit failure.
            let cert_digest = target_cert.certificate.order.digest();
            if attempted_certificates.contains(&cert_digest) {
                return Err(FastPayError::AuthorityInformationUnavailable);
            }
            attempted_certificates.insert(cert_digest);

            // TODO: Eventually the client will store more information, and we could
            // first try to read certificates and parents from a local cache before
            // asking an authority.
            // let input_objects = target_cert.certificate.order.input_objects();

            let order_info = if missing_certificates.is_empty() {
                // Here we cover a corner case due to the nature of using consistent
                // broadcast: it is possible for the client to have a certificate
                // signed by some authority, before the authority has processed the
                // certificate. This can only happen to a certificate for objects
                // not used in another certificicate, hence it can only be the case
                // for the very first certificate we try to sync. For this reason for
                // this one instead of asking for the effects of a previous execution
                // we send the cert for execution. Since execution is idempotent this
                // is ok.

                source_client
                    .handle_confirmation_order(target_cert.clone())
                    .await?
            } else {
                // Unlike the previous case if a certificate created an object that
                // was involved in the processing of another certificate the previous
                // cert must have been processed, so here we just ask for the effects
                // of such an execution.

                source_client
                    .handle_order_info_request(OrderInfoRequest {
                        transaction_digest: cert_digest,
                    })
                    .await?
            };

            // Put back the target cert
            missing_certificates.push(target_cert);
            let signed_effects = &order_info
                .signed_effects
                .ok_or(FastPayError::AuthorityInformationUnavailable)?;

            for returned_digest in &signed_effects.effects.dependencies {
                // We check that we are not processing twice the same certificate, as
                // it would be common if two objects used by one order, were also both
                // mutated by the same preceeding order.
                if !candidate_certificates.contains(returned_digest) {
                    // Add this cert to the set we have processed
                    candidate_certificates.insert(*returned_digest);

                    let inner_order_info = source_client
                        .handle_order_info_request(OrderInfoRequest {
                            transaction_digest: *returned_digest,
                        })
                        .await?;

                    let returned_certificate = inner_order_info
                        .certified_order
                        .ok_or(FastPayError::AuthorityInformationUnavailable)?;

                    // Check & Add it to the list of certificates to sync
                    returned_certificate.check(&self.committee).map_err(|_| {
                        FastPayError::ByzantineAuthoritySuspicion {
                            authority: source_authority,
                        }
                    })?;
                    missing_certificates.push(ConfirmationOrder::new(returned_certificate));
                }
            }
        }

        Ok(())
    }

    /// Sync a certificate to an authority.
    ///
    /// This function infers which authorities have the history related to
    /// a certificate and attempts `retries` number of them, sampled accoding to
    /// stake, in order to bring the destination authority up to date to accept
    /// the certificate. The time devoted to each attempt is bounded by
    /// `timeout_milliseconds`.
    pub async fn sync_certificate_to_authority_with_timeout(
        &self,
        cert: ConfirmationOrder,
        destination_authority: AuthorityName,
        timeout_period: Duration,
        retries: usize,
    ) -> Result<(), FastPayError> {
        // Extract the set of authorities that should have this certificate
        // and its full history. We should be able to use these are source authorities.
        let mut candidate_source_authorties: HashSet<AuthorityName> = cert
            .certificate
            .signatures
            .iter()
            .map(|(name, _)| *name)
            .collect();

        // Sample a `retries` number of distinct authorities by stake.
        let mut source_authorities: Vec<AuthorityName> = Vec::new();
        while source_authorities.len() < retries && !candidate_source_authorties.is_empty() {
            // Here we do rejection sampling.
            //
            // TODO: add a filter parameter to sample, so that we can directly
            //       sample from a subset which is more efficient.
            let sample_authority = self.committee.sample();
            if candidate_source_authorties.contains(sample_authority) {
                candidate_source_authorties.remove(sample_authority);
                source_authorities.push(*sample_authority);
            }
        }

        // Now try to update the destination authority sequentially using
        // the source authorities we have sampled.
        for source_authority in source_authorities {
            // Note: here we could improve this function by passing into the
            //       `sync_authority_source_to_destination` call a cache of
            //       certificates and parents to avoid re-downloading them.
            if timeout(
                timeout_period,
                self.sync_authority_source_to_destination(
                    cert.clone(),
                    source_authority,
                    destination_authority,
                ),
            )
            .await
            .is_ok()
            {
                // If the updates suceeds we return, since there is no need
                // to try other sources.
                return Ok(());
            }

            // If we are here it means that the update failed, either due to the
            // source being faulty or the destination being faulty.
            //
            // TODO: We should probably be keeping a record of suspected faults
            // upon failure to de-prioritize authorities that we have observed being
            // less reliable.
        }

        // Eventually we should add more information to this error about the destination
        // and maybe event the certificiate.
        Err(FastPayError::AuthorityUpdateFailure)
    }

    /// This function takes an initial state, than executes an asynchronous function (FMap) for each
    /// uthority, and folds the results as they become available into the state using an async function (FReduce).
    ///
    /// FMap can do io, and returns a result V. An error there may not be fatal, and could be consumed by the
    /// MReduce function to overall recover from it. This is necessary to ensure byzantine authorities cannot
    /// interupt the logic of this function.
    ///
    /// FReduce returns a result to a ReduceOutput. If the result is Err the function
    /// shortcuts and the Err is returned. An Ok ReduceOutput result can be used to shortcut and return
    /// the resulting state (ReduceOutput::End), continue the folding as new states arrive (ReduceOutput::Continue),
    /// or continue with a timeout maximum waiting time (ReduceOutput::ContinueWithTimeout).
    ///
    /// This function provides a flexible way to communicate with a quorum of authorities, processing and
    /// processing their results into a safe overall result, and also safely allowing operations to continue
    /// past the quorum to ensure all authorities are up to date (up to a timeout).
    async fn quorum_map_then_reduce_with_timeout<'a, S, V, FMap, FReduce>(
        &'a self,
        // The initial state that will be used to fold in values from authorities.
        initial_state: S,
        // The async function used to apply to each authority. It takes an authority name,
        // and authority client parameter and returns a Result<V>.
        map_each_authority: FMap,
        // The async function that takes an accumulated state, and a new result for V from an
        // authority and returns a result to a ReduceOutput state.
        mut reduce_result: FReduce,
        // The initial timeout applied to all
        initial_timeout: Duration,
    ) -> Result<S, FastPayError>
    where
        FMap: FnOnce(AuthorityName, &'a A) -> AsyncResult<'a, V, FastPayError> + Clone,
        FReduce: FnMut(
            S,
            AuthorityName,
            usize,
            Result<V, FastPayError>,
        ) -> AsyncResult<'a, ReduceOutput<S>, FastPayError>,
    {
        // TODO: shuffle here according to stake
        let authority_clients = &self.authority_clients;

        // First, execute in parallel for each authority FMap.
        let mut responses: futures::stream::FuturesUnordered<_> = authority_clients
            .iter()
            .map(|(name, client)| {
                let execute = map_each_authority.clone();
                async move { (*name, execute(*name, client).await) }
            })
            .collect();

        let mut current_timeout = initial_timeout;
        let mut accumulated_state = initial_state;
        // Then, as results become available fold them into the state using FReduce.
        while let Ok(Some((authority_name, result))) =
            timeout(current_timeout, responses.next()).await
        {
            let authority_weight = self.committee.weight(&authority_name);
            accumulated_state =
                match reduce_result(accumulated_state, authority_name, authority_weight, result)
                    .await?
                {
                    // In the first two cases we are told to continue the iteration.
                    ReduceOutput::Continue(state) => state,
                    ReduceOutput::ContinueWithTimeout(state, duration) => {
                        // Adjust the waiting timeout.
                        current_timeout = duration;
                        state
                    }
                    ReduceOutput::End(state) => {
                        // The reducer tells us that we have the result needed. Just return it.
                        return Ok(state);
                    }
                }
        }
        Ok(accumulated_state)
    }

    /// Return all the information in the network about a specific object, including all versions of it
    /// as well as all certificates that lead to the versions and the authorities at that version.
    pub async fn get_object_by_id(
        &self,
        object_id: ObjectID,
        timeout_after_quorum: Duration,
    ) -> Result<
        (
            BTreeMap<
                (ObjectRef, TransactionDigest),
                (Option<Object>, Vec<(AuthorityName, Option<SignedOrder>)>),
            >,
            HashMap<TransactionDigest, CertifiedOrder>,
        ),
        FastPayError,
    > {
        let initial_state = ((0usize, 0usize), Vec::new());
        let threshold = self.committee.quorum_threshold();
        let validity = self.committee.validity_threshold();
        let final_state = self
            .quorum_map_then_reduce_with_timeout(
                initial_state,
                |_name, client| {
                    Box::pin(async move {
                        // Request and return an error if any
                        let request = ObjectInfoRequest::from(object_id);
                        client.handle_object_info_request(request).await
                    })
                },
                |(mut total_stake, mut state), name, weight, result| {
                    Box::pin(async move {
                        // Here we increase the stake counter no matter if we got an error or not. The idea is that a
                        // call to ObjectInfoRequest should suceed for correct authorities no matter what. Therefore
                        // if there is an error it means that we are accessing an incorrect authority. However, an
                        // object is final if it is on 2f+1 good nodes, and any set of 2f+1 intersects with this, so
                        // after we have 2f+1 of stake (good or bad) we should get a response with the object.
                        total_stake.0 += weight;

                        if result.is_err() {
                            // We also keep an error stake counter, and if it is larger than f+1 we return an error,
                            // since either there are too many faulty authorities or we are not connected to the network.
                            total_stake.1 += weight;
                            if total_stake.1 > validity {
                                return Err(FastPayError::TooManyIncorrectAuthorities);
                            }
                        }

                        state.push((name, result));

                        if total_stake.0 < threshold {
                            // While we are under the threshold we wait for a longer time
                            Ok(ReduceOutput::Continue((total_stake, state)))
                        } else {
                            // After we reach threshold we wait for potentially less time.
                            Ok(ReduceOutput::ContinueWithTimeout(
                                (total_stake, state),
                                timeout_after_quorum,
                            ))
                        }
                    })
                },
                // A long timeout before we hear back from a quorum
                Duration::from_secs(60),
            )
            .await?;

        let mut error_list = Vec::new();
        let mut object_map = BTreeMap::<
            (ObjectRef, TransactionDigest),
            (Option<Object>, Vec<(AuthorityName, Option<SignedOrder>)>),
        >::new();
        let mut certificates = HashMap::new();

        for (name, result) in final_state.1 {
            if let Ok(ObjectInfoResponse {
                parent_certificate,
                requested_object_reference,
                object_and_lock,
            }) = result
            {
                // Extract the object_ref and transaction digest that will be used as keys
                let object_ref = if let Some(object_ref) = requested_object_reference {
                    object_ref
                } else {
                    // The object has never been seen on this authority, so we skip
                    continue;
                };

                let (transaction_digest, cert_option) = if let Some(cert) = parent_certificate {
                    (cert.order.digest(), Some(cert))
                } else {
                    (TransactionDigest::genesis(), None)
                };

                // Extract an optional object to be used in the value, note that the object can be
                // None if the object was deleted at this authority
                //
                // NOTE: here we could also be gathering the locked orders to see if we could make a cert.
                let (object_option, signed_order_option) =
                    if let Some(ObjectResponse { object, lock }) = object_and_lock {
                        (Some(object), lock)
                    } else {
                        (None, None)
                    };

                // Update the map with the information from this authority
                let entry = object_map
                    .entry((object_ref, transaction_digest))
                    .or_insert((object_option, Vec::new()));
                entry.1.push((name, signed_order_option));

                if let Some(cert) = cert_option {
                    certificates.insert(cert.order.digest(), cert);
                }
            } else {
                error_list.push((name, result));
            }
        }

        // TODO: return the errors too
        Ok((object_map, certificates))
    }

    /// This function returns a map between object references owned and authorities that hold the objects
    /// at this version, as well as a list of authorities that responsed to the query for the objects owned.
    pub async fn get_all_owned_objects(
        &self,
        address: FastPayAddress,
        timeout_after_quorum: Duration,
    ) -> Result<(BTreeMap<ObjectRef, Vec<AuthorityName>>, Vec<AuthorityName>), FastPayError> {
        let initial_state = (
            (0usize, 0usize),
            BTreeMap::<ObjectRef, Vec<AuthorityName>>::new(),
            Vec::new(),
        );
        let threshold = self.committee.quorum_threshold();
        let validity = self.committee.validity_threshold();
        let (_, object_map, authority_list) = self
            .quorum_map_then_reduce_with_timeout(
                initial_state,
                |_name, client| {
                    // For each authority we ask all objects associated with this address, and return
                    // the result.
                    let inner_address = address;
                    Box::pin(async move {
                        client
                            .handle_account_info_request(AccountInfoRequest::from(inner_address))
                            .await
                    })
                },
                |mut state, name, weight, _result| {
                    Box::pin(async move {
                        // Here we increase the stake counter no matter if we got a correct
                        // response or not. A final order will have effects on 2f+1 so if we
                        // ask any 2f+1 we should get the version of the latest object.
                        state.0 .0 += weight;

                        // For each non error result we get we add the objects to the map
                        // as keys and append the authority that holds them in the values.
                        if let Ok(AccountInfoResponse { object_ids, .. }) = _result {
                            // Also keep a record of all authorities that responded.
                            state.2.push(name);
                            // Update the map.
                            for obj_ref in object_ids {
                                state.1.entry(obj_ref).or_insert_with(Vec::new).push(name);
                            }
                        } else {
                            // We also keep an error weight counter, and if it exceeds 1/3
                            // we return an error as it is likely we do not have enough
                            // evidence to return a correct result.

                            state.0 .1 += weight;
                            if state.0 .1 > validity {
                                return Err(FastPayError::TooManyIncorrectAuthorities);
                            }
                        }

                        if state.0 .0 < threshold {
                            // While we are under the threshold we wait for a longer time
                            Ok(ReduceOutput::Continue(state))
                        } else {
                            // After we reach threshold we wait for potentially less time.
                            Ok(ReduceOutput::ContinueWithTimeout(
                                state,
                                timeout_after_quorum,
                            ))
                        }
                    })
                },
                // A long timeout before we hear back from a quorum
                Duration::from_secs(60),
            )
            .await?;
        Ok((object_map, authority_list))
    }

    /// Ask authorities for the user owned objects. Then download all objects at all versions present
    /// on authorites, along with the certificates preceeding them, and update lagging authorities to
    /// the latest version of the object.
    ///
    /// This function returns all objects, including those that are
    /// no more owned by the user (but were previously owned by the user), as well as a list of
    /// deleted object references.
    pub async fn sync_all_owned_objects(
        &self,
        address: FastPayAddress,
        timeout_after_quorum: Duration,
    ) -> Result<(Vec<Object>, Vec<ObjectRef>), FastPayError> {
        // First get a map of all objects at least a quorum of authorities think we hold.
        let (object_map, _authority_list) = self
            .get_all_owned_objects(address, timeout_after_quorum)
            .await?;

        // We make a list of all versions, in order
        let mut object_latest_version: BTreeMap<ObjectID, Vec<ObjectRef>> = BTreeMap::new();
        for object_ref in object_map.keys() {
            let entry = object_latest_version
                .entry(object_ref.0)
                .or_insert_with(Vec::new);
            entry.push(*object_ref);
            entry.sort();
        }

        let mut active_objects = Vec::new();
        let mut deleted_objects = Vec::new();
        let mut certs_to_sync = BTreeMap::new();
        // We update each object at each authority that does not have it.
        for object_id in object_latest_version.keys() {
            // Authorities to update.
            let mut authorites: HashSet<AuthorityName> = self
                .committee
                .voting_rights
                .iter()
                .map(|(name, _)| *name)
                .collect();

            let (aggregate_object_info, certificates) = self
                .get_object_by_id(*object_id, timeout_after_quorum)
                .await?;

            let mut aggregate_object_info: Vec<_> = aggregate_object_info.into_iter().collect();

            // If more that one version of an object is available, we update all authorities with it.
            while !aggregate_object_info.is_empty() {
                // This will be the very latest object version, because object_ref is ordered this way.
                let ((object_ref, transaction_digest), (object_option, object_authorities)) =
                    aggregate_object_info.pop().unwrap(); // safe due to check above

                // NOTE: Here we must check that the object is indeed an input to this transaction
                //       but for the moment lets do the happy case.

                if !certificates.contains_key(&transaction_digest) {
                    // NOTE: This implies this is a genesis object. We should check that it is.
                    //       We can do this by looking into the genesis, or the object_refs of the genesis.
                    //       Otherwise report the authority as potentially faulty.

                    if let Some(obj) = object_option {
                        active_objects.push(obj);
                    }
                    // Cannot be that the genesis contributes to deleted objects

                    continue;
                }

                let cert = certificates[&transaction_digest].clone(); // safe due to check above.

                // Remove authorities at this version, they will not need to be updated.
                for (name, _signed_order) in object_authorities {
                    authorites.remove(&name);
                }

                // NOTE: Just above we have access to signed orders that have not quite
                //       been processed by enough authorities. We should either return them
                //       to the caller, or -- more in the spirit of this function -- do what
                //       needs to be done to force their processing if this is possible.

                // Add authorities that need to be updated
                let entry = certs_to_sync
                    .entry(cert.order.digest())
                    .or_insert((cert, HashSet::new()));
                entry.1.extend(authorites);

                // Return the latest version of an object, or a deleted object
                match object_option {
                    Some(obj) => active_objects.push(obj),
                    None => deleted_objects.push(object_ref),
                }

                break;
            }
        }

        for (_, (cert, authorities)) in certs_to_sync {
            for name in authorities {
                // For each certificate authority pair run a sync to upate this authority to this
                // certificate.
                // NOTE: this is right now done sequentially, we should do them in parallel using
                //       the usual FuturesUnordered.
                let _result = self
                    .sync_certificate_to_authority_with_timeout(
                        ConfirmationOrder::new(cert.clone()),
                        name,
                        timeout_after_quorum,
                        1,
                    )
                    .await;

                // TODO: collect errors and propagate them to the right place
            }
        }

        Ok((active_objects, deleted_objects))
    }

    #[cfg(test)]
    async fn request_certificate(
        &self,
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
        };
        let mut authority_clients = self.authority_clients.clone();
        let numbers: futures::stream::FuturesUnordered<_> = authority_clients
            .iter_mut()
            .map(|(name, client)| {
                let fut = client.handle_object_info_request(request.clone());
                async move {
                    match fut.await {
                        Ok(info) => info.object().map(|obj| (*name, obj.version())),
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
    ) -> Option<(Authenticator, SequenceNumber)> {
        let request = ObjectInfoRequest {
            object_id,
            request_sequence_number: None,
        };
        let authority_clients = self.authority_clients.clone();
        let numbers: futures::stream::FuturesUnordered<_> = authority_clients
            .iter()
            .map(|(name, client)| {
                let fut = client.handle_object_info_request(request.clone());
                async move {
                    match fut.await {
                        Ok(ObjectInfoResponse {
                            object_and_lock: Some(ObjectResponse { object, .. }),
                            ..
                        }) => Some((*name, Some((object.owner, object.version())))),
                        _ => None,
                    }
                }
            })
            .collect();
        self.committee.get_strong_majority_lower_bound(
            numbers.filter_map(|x| async move { x }).collect().await,
        )
    }

    /// Execute a sequence of actions in parallel for a quorum of authorities.
    async fn communicate_with_quorum<'a, V, F>(&'a self, execute: F) -> Result<Vec<V>, FastPayError>
    where
        F: Fn(AuthorityName, &'a A) -> AsyncResult<'a, V, FastPayError> + Clone,
    {
        let committee = &self.committee;
        let authority_clients = &self.authority_clients;
        let mut responses: futures::stream::FuturesUnordered<_> = authority_clients
            .iter()
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

    /// Broadcast transaction order on each authority client.
    async fn broadcast_tx_order(
        &self,
        order: Order,
    ) -> Result<(OrderInfoResponse, CertifiedOrder), anyhow::Error> {
        let committee = self.committee.clone();
        // We are not broadcasting any confirmation orders, so certificates_to_broadcast vec is empty
        let (_confirmation_responses, order_votes) = self
            .broadcast_and_execute(Vec::new(), |name, authority| {
                let order = order.clone();
                let committee = committee.clone();
                Box::pin(async move {
                    match authority.handle_order(order).await {
                        // Check if the response is okay
                        Ok(response) =>
                        // Verify we have a signed order
                        {
                            match response.clone().signed_order {
                                Some(inner_signed_order) => {
                                    fp_ensure!(
                                        inner_signed_order.authority == name,
                                        FastPayError::ErrorWhileProcessingTransactionOrder {
                                            err: "Signed by unexpected authority".to_string()
                                        }
                                    );
                                    inner_signed_order.check(&committee)?;
                                    Ok(response)
                                }
                                None => Err(FastPayError::ErrorWhileProcessingTransactionOrder {
                                    err: "Invalid order response".to_string(),
                                }),
                            }
                        }
                        Err(err) => Err(err),
                    }
                })
            })
            .await?;
        // Collate the signatures
        // If we made it here, values are safe
        let signatures = order_votes
            .iter()
            .map(|vote| {
                (
                    vote.signed_order.as_ref().unwrap().authority,
                    vote.signed_order.as_ref().unwrap().signature,
                )
            })
            .collect::<Vec<_>>();

        let certificate = CertifiedOrder { order, signatures };
        // Certificate is valid because
        // * `communicate_with_quorum` ensured a sufficient "weight" of (non-error) answers were returned by authorities.
        // * each answer is a vote signed by the expected authority.

        // Assume all responses are same. Pick first
        Ok((order_votes.get(0).unwrap().clone(), certificate))
    }

    /// Broadcast missing confirmation orders and execute provided authority action on each authority.
    // BUG(https://github.com/MystenLabs/fastnft/issues/290): This logic for
    // updating an authority that is behind is not correct, since we now have
    // potentially many dependencies that need to be satisfied, not just a
    // list.
    async fn broadcast_and_execute<'a, V, F: 'a>(
        &'a self,
        certificates_to_broadcast: Vec<CertifiedOrder>,
        action: F,
    ) -> Result<(Vec<(CertifiedOrder, OrderInfoResponse)>, Vec<V>), anyhow::Error>
    where
        F: Fn(AuthorityName, &'a A) -> AsyncResult<'a, V, FastPayError> + Send + Sync + Copy,
        V: Clone,
    {
        let result = self
            .communicate_with_quorum(|name, client| {
                let certificates_to_broadcast = certificates_to_broadcast.clone();
                Box::pin(async move {
                    let mut responses = vec![];
                    for certificate in certificates_to_broadcast {
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

        let action_results = result.iter().map(|(_, result)| result.clone()).collect();

        // Assume all responses are the same, pick the first one.
        let order_response = result
            .iter()
            .map(|(response, _)| response.clone())
            .next()
            .unwrap_or_default();

        Ok((order_response, action_results))
    }

    pub async fn update_authority_certificates(
        &mut self,
        sender: FastPayAddress,
        inputs: &[InputObjectKind],
        known_certificates: Vec<((ObjectID, SequenceNumber), FastPayResult<CertifiedOrder>)>,
    ) -> FastPayResult<Vec<Vec<(CertifiedOrder, OrderInfoResponse)>>> {
        let requester = CertificateRequester::new(
            self.committee.clone(),
            self.authority_clients.values().cloned().collect(),
            Some(sender),
        );

        let (_, handle) = Downloader::start(requester, known_certificates);
        self.communicate_with_quorum(|_name, client| {
            let mut handle = handle.clone();
            Box::pin(async move {
                // Sync certificate with authority
                // Figure out which certificates this authority is missing.
                let mut responses = Vec::new();
                let mut missing_certificates = Vec::new();
                for input_kind in inputs {
                    let object_id = input_kind.object_id();
                    let target_sequence_number = input_kind.version();
                    let request = ObjectInfoRequest {
                        object_id,
                        request_sequence_number: None,
                    };
                    let response = client.handle_object_info_request(request).await?;

                    let current_sequence_number = response
                        .object_and_lock
                        .ok_or(FastPayError::ObjectNotFound { object_id })?
                        .object
                        .version();

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
                for certificate in missing_certificates {
                    responses.push((
                        certificate.clone(),
                        client
                            .handle_confirmation_order(ConfirmationOrder::new(certificate))
                            .await?,
                    ));
                }
                Ok(responses)
            })
        })
        .await
    }

    /// Broadcast confirmation orders.
    /// The corresponding sequence numbers should be consecutive and increasing.
    pub async fn broadcast_confirmation_orders(
        &self,
        certificates_to_broadcast: Vec<CertifiedOrder>,
    ) -> Result<Vec<(CertifiedOrder, OrderInfoResponse)>, anyhow::Error> {
        self.broadcast_and_execute(certificates_to_broadcast, |_, _| Box::pin(async { Ok(()) }))
            .await
            .map(|(responses, _)| responses)
    }

    pub async fn request_certificates_from_authority(
        &self,
        known_sequence_numbers_map: BTreeMap<(ObjectID, SequenceNumber), HashSet<SequenceNumber>>,
    ) -> Result<BTreeMap<ObjectID, Vec<CertifiedOrder>>, FastPayError> {
        let mut sent_certificates: BTreeMap<ObjectID, Vec<CertifiedOrder>> = BTreeMap::new();

        for ((object_id, next_sequence_number), known_sequence_numbers) in
            known_sequence_numbers_map
        {
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

    pub async fn execute_transaction(
        &self,
        order: &Order,
    ) -> Result<(CertifiedOrder, OrderInfoResponse), anyhow::Error> {
        let new_certificate = self.execute_transaction_without_confirmation(order).await?;

        // Confirm transfer certificate if specified.
        let responses = self
            .broadcast_confirmation_orders(vec![new_certificate.clone()])
            .await?;

        // Find response for the current order from all the returned order responses.
        let (_, response) = responses
            .into_iter()
            .find(|(cert, _)| cert.order == new_certificate.order)
            .ok_or(FastPayError::ErrorWhileRequestingInformation)?;

        Ok((new_certificate, response))
    }

    /// Execute (or retry) an order without confirmation. Update local object states using newly created certificate.
    async fn execute_transaction_without_confirmation(
        &self,
        order: &Order,
    ) -> Result<CertifiedOrder, anyhow::Error> {
        let result = self.broadcast_tx_order(order.clone()).await;

        let (_, new_sent_certificate) = result?;
        assert_eq!(&new_sent_certificate.order, order);
        // TODO: Verify that we don't need to update client objects here based on order_info_responses,
        // but can do it at the caller site.

        Ok(new_sent_certificate)
    }

    // TODO: This is incomplete at the moment.
    // A complete algorithm is being introduced in
    // https://github.com/MystenLabs/fastnft/pull/336.
    pub async fn download_own_object_ids_from_random_authority(
        &self,
        address: FastPayAddress,
    ) -> Result<(AuthorityName, Vec<ObjectRef>), FastPayError> {
        let request = AccountInfoRequest { account: address };
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

    pub async fn get_object_info_execute(
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

    /// Given a list of object refs, download the objects.
    pub fn fetch_objects_from_authorities(
        &self,
        object_refs: BTreeSet<ObjectRef>,
    ) -> Receiver<FastPayResult<Object>> {
        let (sender, receiver) = tokio::sync::mpsc::channel(OBJECT_DOWNLOAD_CHANNEL_BOUND);
        for object_ref in object_refs {
            let sender = sender.clone();
            tokio::spawn(Self::fetch_one_object(
                self.authority_clients.clone(),
                object_ref,
                AUTHORITY_REQUEST_TIMEOUT,
                sender,
            ));
        }
        // Close unused channel
        drop(sender);
        receiver
    }

    /// This function fetches one object at a time, and sends back the result over the channel
    /// The object ids are also returned so the caller can determine which fetches failed
    /// NOTE: This function assumes all authorities are honest
    async fn fetch_one_object(
        authority_clients: BTreeMap<PublicKeyBytes, A>,
        object_ref: ObjectRef,
        timeout: Duration,
        sender: tokio::sync::mpsc::Sender<Result<Object, FastPayError>>,
    ) {
        let object_id = object_ref.0;
        // Prepare the request
        let request = ObjectInfoRequest {
            object_id,
            request_sequence_number: None,
        };

        // For now assume all authorities. Assume they're all honest
        // This assumption is woeful, and should be fixed
        // TODO: https://github.com/MystenLabs/fastnft/issues/320
        let results = future::join_all(authority_clients.iter().map(|(_, ac)| {
            tokio::time::timeout(timeout, ac.handle_object_info_request(request.clone()))
        }))
        .await;

        fn obj_fetch_err(id: ObjectID, err: &str) -> Result<Object, FastPayError> {
            Err(FastPayError::ObjectFetchFailed {
                object_id: id,
                err: err.to_owned(),
            })
        }

        let mut ret_val: Result<Object, FastPayError> = Err(FastPayError::ObjectFetchFailed {
            object_id: object_ref.0,
            err: "No authority returned object".to_string(),
        });
        // Find the first non-error value
        // There are multiple reasons why we might not have an object
        // We can timeout, or the authority returns an error or simply no object
        // When we get an object back, it also might not match the digest we want
        for result in results {
            // Check if the result of the call is successful
            ret_val = match result {
                Ok(res) => match res {
                    // Check if the authority actually had an object
                    Ok(resp) => match resp.object_and_lock {
                        Some(o) => {
                            // Check if this is the the object we want
                            if o.object.digest() == object_ref.2 {
                                Ok(o.object)
                            } else {
                                obj_fetch_err(object_id, "Object digest mismatch")
                            }
                        }
                        None => obj_fetch_err(object_id, "object_and_lock is None"),
                    },
                    // Something in FastX failed
                    Err(e) => Err(e),
                },
                // Took too long
                Err(e) => obj_fetch_err(object_id, e.to_string().as_str()),
            };
            // We found a value
            if ret_val.is_ok() {
                break;
            }
        }
        sender
            .send(ret_val)
            .await
            .expect("Cannot send object on channel after object fetch attempt");
    }
}
