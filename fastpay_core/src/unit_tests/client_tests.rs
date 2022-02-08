// Copyright (c) Facebook, Inc. and its affiliates.
// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::same_item_push)] // get_key_pair returns random elements

use super::*;
use crate::authority::{AuthorityState, AuthorityStore};
use crate::client::client_store::ClientStore;
use crate::client::{Client, ClientState};
use fastx_types::{
    object::{Object, GAS_VALUE_FOR_TESTING, OBJECT_START_VERSION},
    FASTX_FRAMEWORK_ADDRESS,
};
use futures::lock::Mutex;
use move_core_types::ident_str;
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryInto,
    sync::Arc,
};
use tokio::runtime::Runtime;
use typed_store::Map;

use fastx_types::error::FastPayError::ObjectNotFound;
use move_core_types::account_address::AccountAddress;
use std::env;
use std::fs;

// Only relevant in a ser/de context : the `CertifiedOrder` for a transaction is not unique
fn compare_certified_orders(o1: &CertifiedOrder, o2: &CertifiedOrder) {
    assert_eq!(o1.order.digest(), o2.order.digest());
    // in this ser/de context it's relevant to compare signatures
    assert_eq!(o1.signatures, o2.signatures);
}

pub fn system_maxfiles() -> usize {
    fdlimit::raise_fd_limit().unwrap_or(256u64) as usize
}

fn max_files_client_tests() -> i32 {
    (system_maxfiles() / 8).try_into().unwrap()
}

#[derive(Clone)]
struct LocalAuthorityClient(Arc<Mutex<AuthorityState>>);

#[async_trait]
impl AuthorityAPI for LocalAuthorityClient {
    async fn handle_order(&self, order: Order) -> Result<OrderInfoResponse, FastPayError> {
        let state = self.0.clone();
        let result = state.lock().await.handle_order(order).await;
        result
    }

    async fn handle_confirmation_order(
        &self,
        order: ConfirmationOrder,
    ) -> Result<OrderInfoResponse, FastPayError> {
        let state = self.0.clone();
        let result = state.lock().await.handle_confirmation_order(order).await;
        result
    }

    async fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, FastPayError> {
        let state = self.0.clone();

        let result = state
            .lock()
            .await
            .handle_account_info_request(request)
            .await;
        result
    }

    async fn handle_object_info_request(
        &self,
        request: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, FastPayError> {
        let state = self.0.clone();
        let x = state.lock().await.handle_object_info_request(request).await;
        x
    }

    /// Handle Object information requests for this account.
    async fn handle_order_info_request(
        &self,
        request: OrderInfoRequest,
    ) -> Result<OrderInfoResponse, FastPayError> {
        let state = self.0.clone();

        let result = state.lock().await.handle_order_info_request(request).await;
        result
    }
}

impl LocalAuthorityClient {
    fn new(state: AuthorityState) -> Self {
        Self(Arc::new(Mutex::new(state)))
    }
}

#[cfg(test)]
async fn init_local_authorities(
    count: usize,
) -> (BTreeMap<AuthorityName, LocalAuthorityClient>, Committee) {
    let mut key_pairs = Vec::new();
    let mut voting_rights = BTreeMap::new();
    for _ in 0..count {
        let key_pair = get_key_pair();
        voting_rights.insert(key_pair.0, 1);
        key_pairs.push(key_pair);
    }
    let committee = Committee::new(voting_rights);

    let mut clients = BTreeMap::new();
    for (address, secret) in key_pairs {
        // Random directory for the DB
        let dir = env::temp_dir();
        let path = dir.join(format!("DB_{:?}", ObjectID::random()));
        fs::create_dir(&path).unwrap();

        let mut opts = rocksdb::Options::default();
        opts.set_max_open_files(max_files_client_tests());
        let store = Arc::new(AuthorityStore::open(path, Some(opts)));

        let state = AuthorityState::new_with_genesis_modules(
            committee.clone(),
            address,
            Box::pin(secret),
            store,
        )
        .await;
        clients.insert(address, LocalAuthorityClient::new(state));
    }
    (clients, committee)
}

#[cfg(test)]
fn init_local_authorities_bad_1(
    count: usize,
) -> (BTreeMap<AuthorityName, LocalAuthorityClient>, Committee) {
    let mut key_pairs = Vec::new();
    let mut voting_rights = BTreeMap::new();
    for i in 0..count {
        let key_pair = get_key_pair();
        voting_rights.insert(key_pair.0, 1);
        if i + 1 < (count + 2) / 3 {
            // init 1 authority with a bad keypair
            key_pairs.push(get_key_pair());
        } else {
            key_pairs.push(key_pair);
        }
    }
    let committee = Committee::new(voting_rights);

    let mut clients = BTreeMap::new();
    for (address, secret) in key_pairs {
        // Random directory
        let dir = env::temp_dir();
        let path = dir.join(format!("DB_{:?}", ObjectID::random()));
        fs::create_dir(&path).unwrap();

        let mut opts = rocksdb::Options::default();
        opts.set_max_open_files(max_files_client_tests());
        let store = Arc::new(AuthorityStore::open(path, Some(opts)));
        let state = AuthorityState::new_without_genesis_for_testing(
            committee.clone(),
            address,
            Box::pin(secret),
            store,
        );
        clients.insert(address, LocalAuthorityClient::new(state));
    }
    (clients, committee)
}

#[cfg(test)]
fn make_client(
    authority_clients: BTreeMap<AuthorityName, LocalAuthorityClient>,
    committee: Committee,
) -> ClientState<LocalAuthorityClient> {
    let (address, secret) = get_key_pair();
    let pb_secret = Box::pin(secret);
    ClientState::new(
        env::temp_dir().join(format!("CLIENT_DB_{:?}", ObjectID::random())),
        address,
        pb_secret,
        committee,
        authority_clients,
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap()
}

#[cfg(test)]
async fn fund_account_with_same_objects(
    authorities: Vec<&LocalAuthorityClient>,
    client: &mut ClientState<LocalAuthorityClient>,
    object_ids: Vec<ObjectID>,
) -> HashMap<AccountAddress, Object> {
    let objs: Vec<_> = (0..authorities.len()).map(|_| object_ids.clone()).collect();
    fund_account(authorities, client, objs).await
}

#[cfg(test)]
async fn fund_account(
    authorities: Vec<&LocalAuthorityClient>,
    client: &mut ClientState<LocalAuthorityClient>,
    object_ids: Vec<Vec<ObjectID>>,
) -> HashMap<AccountAddress, Object> {
    let mut created_objects = HashMap::new();
    for (authority, object_ids) in authorities.into_iter().zip(object_ids.into_iter()) {
        for object_id in object_ids {
            let object = Object::with_id_owner_for_testing(object_id, client.address());
            let client_ref = authority.0.as_ref().try_lock().unwrap();
            created_objects.insert(object_id, object.clone());

            let object_ref: ObjectRef = (object_id, 0.into(), object.digest());

            client_ref.init_order_lock(object_ref).await;
            client_ref.insert_object(object).await;
            client
                .store()
                .object_sequence_numbers
                .insert(&object_id, &SequenceNumber::new())
                .unwrap();
            client
                .store()
                .object_refs
                .insert(&object_id, &object_ref)
                .unwrap();
        }
    }
    created_objects
}

#[cfg(test)]
async fn init_local_client_state(
    object_ids: Vec<Vec<ObjectID>>,
) -> ClientState<LocalAuthorityClient> {
    let (authority_clients, committee) = init_local_authorities(object_ids.len()).await;
    let mut client = make_client(authority_clients.clone(), committee);
    fund_account(
        authority_clients.values().collect(),
        &mut client,
        object_ids,
    )
    .await;
    client
}

#[cfg(test)]
async fn init_local_client_state_with_bad_authority(
    object_ids: Vec<Vec<ObjectID>>,
) -> ClientState<LocalAuthorityClient> {
    let (authority_clients, committee) = init_local_authorities_bad_1(object_ids.len());
    let mut client = make_client(authority_clients.clone(), committee);
    fund_account(
        authority_clients.values().collect(),
        &mut client,
        object_ids,
    )
    .await;
    client
}

#[test]
fn test_get_strong_majority_owner() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let object_id_1 = ObjectID::random();
        let object_id_2 = ObjectID::random();
        let authority_objects = vec![
            vec![object_id_1],
            vec![object_id_1, object_id_2],
            vec![object_id_1, object_id_2],
            vec![object_id_1, object_id_2],
        ];
        let client = init_local_client_state(authority_objects).await;
        assert_eq!(
            client
                .authorities()
                .get_strong_majority_owner(object_id_1)
                .await,
            Some((
                Authenticator::Address(client.address()),
                SequenceNumber::from(0)
            ))
        );
        assert_eq!(
            client
                .authorities()
                .get_strong_majority_owner(object_id_2)
                .await,
            Some((
                Authenticator::Address(client.address()),
                SequenceNumber::from(0)
            ))
        );

        let object_id_1 = ObjectID::random();
        let object_id_2 = ObjectID::random();
        let object_id_3 = ObjectID::random();
        let authority_objects = vec![
            vec![object_id_1],
            vec![object_id_2, object_id_3],
            vec![object_id_3, object_id_2],
            vec![object_id_3],
        ];
        let client = init_local_client_state(authority_objects).await;
        assert_eq!(
            client
                .authorities()
                .get_strong_majority_owner(object_id_1)
                .await,
            None
        );
        assert_eq!(
            client
                .authorities()
                .get_strong_majority_owner(object_id_2)
                .await,
            None
        );
        assert_eq!(
            client
                .authorities()
                .get_strong_majority_owner(object_id_3)
                .await,
            Some((
                Authenticator::Address(client.address()),
                SequenceNumber::from(0)
            ))
        );
    });
}

#[test]
fn test_initiating_valid_transfer() {
    let rt = Runtime::new().unwrap();
    let (recipient, _) = get_key_pair();
    let object_id_1 = ObjectID::random();
    let object_id_2 = ObjectID::random();
    let gas_object = ObjectID::random();
    let authority_objects = vec![
        vec![object_id_1, gas_object],
        vec![object_id_1, object_id_2, gas_object],
        vec![object_id_1, object_id_2, gas_object],
        vec![object_id_1, object_id_2, gas_object],
    ];

    let mut sender = rt.block_on(init_local_client_state(authority_objects));
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id_1)),
        Some((
            Authenticator::Address(sender.address()),
            SequenceNumber::from(0)
        ))
    );
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id_2)),
        Some((
            Authenticator::Address(sender.address()),
            SequenceNumber::from(0)
        ))
    );
    let (certificate, _) = rt
        .block_on(sender.transfer_object(object_id_1, gas_object, recipient))
        .unwrap();
    assert_eq!(
        sender.next_sequence_number(&object_id_1),
        Err(FastPayError::ObjectNotFound {
            object_id: object_id_1
        })
    );
    assert!(sender.store().pending_orders.is_empty());
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id_1)),
        Some((Authenticator::Address(recipient), SequenceNumber::from(1)))
    );
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id_2)),
        Some((
            Authenticator::Address(sender.address()),
            SequenceNumber::from(0)
        ))
    );
    // valid since our test authority should not update its certificate set
    compare_certified_orders(
        &rt.block_on(sender.authorities().request_certificate(
            sender.address(),
            object_id_1,
            SequenceNumber::from(0),
        ))
        .unwrap(),
        &certificate,
    );
}

#[test]
fn test_initiating_valid_transfer_despite_bad_authority() {
    let rt = Runtime::new().unwrap();
    let (recipient, _) = get_key_pair();
    let object_id = ObjectID::random();
    let gas_object = ObjectID::random();
    let authority_objects = vec![
        vec![object_id, gas_object],
        vec![object_id, gas_object],
        vec![object_id, gas_object],
        vec![object_id, gas_object],
    ];
    let mut sender = rt.block_on(init_local_client_state_with_bad_authority(
        authority_objects,
    ));
    let (certificate, _) = rt
        .block_on(sender.transfer_object(object_id, gas_object, recipient))
        .unwrap();
    assert_eq!(
        sender.next_sequence_number(&object_id),
        Err(ObjectNotFound { object_id })
    );
    assert!(sender.store().pending_orders.is_empty());
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id)),
        Some((Authenticator::Address(recipient), SequenceNumber::from(1)))
    );
    // valid since our test authority shouldn't update its certificate set
    compare_certified_orders(
        &rt.block_on(sender.authorities().request_certificate(
            sender.address(),
            object_id,
            SequenceNumber::from(0),
        ))
        .unwrap(),
        &certificate,
    );
}

#[test]
fn test_initiating_transfer_low_funds() {
    let rt = Runtime::new().unwrap();
    let (recipient, _) = get_key_pair();
    let object_id_1 = ObjectID::random();
    let object_id_2 = ObjectID::random();
    let gas_object = ObjectID::random();
    let authority_objects = vec![
        vec![object_id_1, gas_object],
        vec![object_id_1, gas_object],
        vec![object_id_1, object_id_2, gas_object],
        vec![object_id_1, object_id_2, gas_object],
    ];
    let mut sender = rt.block_on(init_local_client_state(authority_objects));
    assert!(rt
        .block_on(sender.transfer_object(object_id_2, gas_object, recipient))
        .is_err());
    // Trying to overspend does not block an account.
    assert_eq!(
        sender.next_sequence_number(&object_id_2),
        Ok(SequenceNumber::from(0))
    );
    // assert_eq!(sender.pending_transfer, None);
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id_1)),
        Some((
            Authenticator::Address(sender.address()),
            SequenceNumber::from(0)
        )),
    );
    assert_eq!(
        rt.block_on(sender.authorities().get_strong_majority_owner(object_id_2)),
        None,
    );
}

#[tokio::test]
async fn test_bidirectional_transfer() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let mut client2 = make_client(authority_clients.clone(), committee);

    let object_id = ObjectID::random();
    let gas_object1 = ObjectID::random();
    let gas_object2 = ObjectID::random();

    fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![object_id, gas_object1],
    )
    .await;
    fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client2,
        vec![gas_object2],
    )
    .await;

    // Confirm client1 have ownership of the object.
    assert_eq!(
        client1
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client1.address()),
            SequenceNumber::from(0)
        ))
    );
    // Confirm client2 doesn't have ownership of the object.
    assert_eq!(
        client2
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client1.address()),
            SequenceNumber::from(0)
        ))
    );
    // Transfer object to client2.
    let (certificate, _) = client1
        .transfer_object(object_id, gas_object1, client2.address())
        .await
        .unwrap();

    assert!(client1.store().pending_orders.is_empty());
    // Confirm client1 lose ownership of the object.
    assert_eq!(
        client1
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client2.address()),
            SequenceNumber::from(1)
        ))
    );
    // Confirm client2 acquired ownership of the object.
    assert_eq!(
        client2
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client2.address()),
            SequenceNumber::from(1)
        ))
    );

    // Confirm certificate is consistent between authorities and client.
    // valid since our test authority should not update its certificate set
    compare_certified_orders(
        &client1
            .authorities()
            .request_certificate(client1.address(), object_id, SequenceNumber::from(0))
            .await
            .unwrap(),
        &certificate,
    );

    // Update client2's local object data.
    client2.receive_object(&certificate).await.unwrap();

    // Confirm sequence number are consistent between clients.
    assert_eq!(
        client2
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client2.address()),
            SequenceNumber::from(1)
        ))
    );

    // Transfer the object back to Client1
    client2
        .transfer_object(object_id, gas_object2, client1.address())
        .await
        .unwrap();

    assert!((client2.store().pending_orders.is_empty()));

    // Confirm client2 lose ownership of the object.
    assert_eq!(
        client2
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client1.address()),
            SequenceNumber::from(2)
        ))
    );
    assert_eq!(
        client2
            .authorities()
            .get_strong_majority_sequence_number(object_id)
            .await,
        SequenceNumber::from(2)
    );
    // Confirm client1 acquired ownership of the object.
    assert_eq!(
        client1
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client1.address()),
            SequenceNumber::from(2)
        ))
    );

    // Should fail if Client 2 double spend the object
    assert!(client2
        .transfer_object(object_id, gas_object2, client1.address(),)
        .await
        .is_err());
}

#[test]
fn test_client_state_sync() {
    let rt = Runtime::new().unwrap();

    let object_ids = (0..20)
        .map(|_| ObjectID::random())
        .collect::<Vec<ObjectID>>();
    let authority_objects = (0..10).map(|_| object_ids.clone()).collect();

    let mut sender = rt.block_on(init_local_client_state(authority_objects));

    let old_object_ids: BTreeMap<_, _> = sender.store().object_sequence_numbers.iter().collect();
    let old_certificates: BTreeMap<_, _> = sender.store().certificates.iter().collect();

    // Remove all client-side data
    sender.store().object_sequence_numbers.clear().unwrap();
    sender.store().certificates.clear().unwrap();
    sender.store().object_refs.clear().unwrap();
    assert!(rt.block_on(sender.get_owned_objects()).is_empty());

    // Sync client state
    rt.block_on(sender.sync_client_state()).unwrap();

    // Confirm data are the same after sync
    assert!(!rt.block_on(sender.get_owned_objects()).is_empty());
    assert_eq!(
        &old_object_ids,
        &sender.store().object_sequence_numbers.iter().collect()
    );
    for tx_digest in old_certificates.keys() {
        // valid since our test authority should not lead us to download new certs
        compare_certified_orders(
            old_certificates.get(tx_digest).unwrap(),
            &sender.store().certificates.get(tx_digest).unwrap().unwrap(),
        );
    }
}

#[tokio::test]
async fn test_client_state_sync_with_transferred_object() {
    let (authority_clients, committee) = init_local_authorities(1).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let mut client2 = make_client(authority_clients.clone(), committee);

    let object_id = ObjectID::random();
    let gas_object_id = ObjectID::random();

    let authority_objects = vec![vec![object_id, gas_object_id]];

    fund_account(
        authority_clients.values().collect(),
        &mut client1,
        authority_objects,
    )
    .await;

    // Transfer object to client2.
    client1
        .transfer_object(object_id, gas_object_id, client2.address())
        .await
        .unwrap();

    // Confirm client2 acquired ownership of the object.
    assert_eq!(
        client2
            .authorities()
            .get_strong_majority_owner(object_id)
            .await,
        Some((
            Authenticator::Address(client2.address()),
            SequenceNumber::from(1)
        ))
    );

    // Client 2's local object_id and cert should be empty before sync
    assert!(client2.get_owned_objects().await.is_empty());
    assert!(client2.store().object_sequence_numbers.is_empty());
    assert!(&client2.store().certificates.is_empty());

    // Sync client state
    client2.sync_client_state().await.unwrap();

    // Confirm client 2 received the new object id and cert
    assert_eq!(1, client2.get_owned_objects().await.len());
    assert_eq!(1, client2.store().object_sequence_numbers.iter().count());
    assert_eq!(1, client2.store().certificates.iter().count());
}

#[tokio::test]
async fn test_client_certificate_state() {
    let number_of_authorities = 1;
    let (authority_clients, committee) = init_local_authorities(number_of_authorities).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let mut client2 = make_client(authority_clients.clone(), committee);

    let object_id_1 = ObjectID::random();
    let object_id_2 = ObjectID::random();
    let gas_object_id_1 = ObjectID::random();
    let gas_object_id_2 = ObjectID::random();

    let client1_objects = vec![object_id_1, object_id_2, gas_object_id_1];
    let client2_objects = vec![gas_object_id_2];

    let client1_objects: Vec<Vec<ObjectID>> = (0..number_of_authorities)
        .map(|_| client1_objects.clone())
        .collect();

    let client2_objects: Vec<Vec<ObjectID>> = (0..number_of_authorities)
        .map(|_| client2_objects.clone())
        .collect();

    fund_account(
        authority_clients.values().collect(),
        &mut client1,
        client1_objects,
    )
    .await;

    fund_account(
        authority_clients.values().collect(),
        &mut client2,
        client2_objects,
    )
    .await;

    // Transfer object to client2.
    client1
        .transfer_object(object_id_1, gas_object_id_1, client2.address())
        .await
        .unwrap();
    client1
        .transfer_object(object_id_2, gas_object_id_1, client2.address())
        .await
        .unwrap();
    // Should have 2 certs after 2 transfer
    assert_eq!(2, client1.store().certificates.iter().count());
    // Only gas_object left in account, so object_certs link should only have 1 entry
    assert_eq!(1, client1.store().object_certs.iter().count());
    // it should have 2 certificates associated with the gas object
    assert!(client1
        .store()
        .object_certs
        .contains_key(&gas_object_id_1)
        .unwrap());
    assert_eq!(
        2,
        client1
            .store()
            .object_certs
            .get(&gas_object_id_1)
            .unwrap()
            .unwrap()
            .len()
    );
    // Sequence number should be 2 for gas object after 2 mutation.
    assert_eq!(
        Ok(SequenceNumber::from(2)),
        client1.next_sequence_number(&gas_object_id_1)
    );

    client2.sync_client_state().await.unwrap();

    // Client 2 should retrieve 2 certificates for the 2 transactions after sync
    assert_eq!(2, client2.store().certificates.iter().count());
    assert!(client2
        .store()
        .object_certs
        .contains_key(&object_id_1)
        .unwrap());
    assert!(client2
        .store()
        .object_certs
        .contains_key(&object_id_2)
        .unwrap());
    assert_eq!(
        1,
        client2
            .store()
            .object_certs
            .get(&object_id_1)
            .unwrap()
            .unwrap()
            .len()
    );
    assert_eq!(
        1,
        client2
            .store()
            .object_certs
            .get(&object_id_2)
            .unwrap()
            .unwrap()
            .len()
    );
    // Sequence number for object 1 and 2 should be 1 after 1 mutation.
    assert_eq!(
        Ok(SequenceNumber::from(1)),
        client2.next_sequence_number(&object_id_1)
    );
    assert_eq!(
        Ok(SequenceNumber::from(1)),
        client2.next_sequence_number(&object_id_2)
    );
    // Transfer object 2 back to client 1.
    client2
        .transfer_object(object_id_2, gas_object_id_2, client1.address())
        .await
        .unwrap();

    assert_eq!(3, client2.store().certificates.iter().count());
    assert!(client2
        .store()
        .object_certs
        .contains_key(&object_id_1)
        .unwrap());
    assert!(!client2
        .store()
        .object_certs
        .contains_key(&object_id_2)
        .unwrap());
    assert!(client2
        .store()
        .object_certs
        .contains_key(&gas_object_id_2)
        .unwrap());
    assert_eq!(
        1,
        client2
            .store()
            .object_certs
            .get(&object_id_1)
            .unwrap()
            .unwrap()
            .len()
    );
    assert_eq!(
        1,
        client2
            .store()
            .object_certs
            .get(&gas_object_id_2)
            .unwrap()
            .unwrap()
            .len()
    );

    client1.sync_client_state().await.unwrap();

    assert_eq!(3, client1.store().certificates.iter().count());
    assert!(client1
        .store()
        .object_certs
        .contains_key(&object_id_2)
        .unwrap());
    assert_eq!(
        2,
        client1
            .store()
            .object_certs
            .get(&object_id_2)
            .unwrap()
            .unwrap()
            .len()
    );
}

#[tokio::test]
async fn test_move_calls_object_create() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee);

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    // Check effects are good
    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;
    // Status flag should be success
    assert_eq!(order_effects.status, ExecutionStatus::Success);
    // Nothing should be deleted during a creation
    assert!(order_effects.deleted.is_empty());
    // A new object is created.
    assert_eq!(
        (order_effects.created.len(), order_effects.mutated.len()),
        (1, 0)
    );
    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);
}

#[tokio::test]
async fn test_move_calls_object_transfer() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let client2 = make_client(authority_clients.clone(), committee);

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;

    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    // Get the object created from the call
    let (new_obj_ref, _) = order_effects.created[0];
    // Fetch the full object
    let new_obj = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    gas_object_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    let pure_args = vec![bcs::to_bytes(&client2.address().to_vec()).unwrap()];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("transfer").to_owned(),
            Vec::new(),
            gas_object_ref,
            vec![new_obj.object().unwrap().to_object_reference()],
            pure_args,
            GAS_VALUE_FOR_TESTING / 2,
        )
        .await;

    // Check effects are good
    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;
    // Status flag should be success
    assert_eq!(order_effects.status, ExecutionStatus::Success);
    // Nothing should be deleted during a transfer
    assert!(order_effects.deleted.is_empty());
    // The object being transfered will be in mutated.
    assert_eq!(order_effects.mutated.len(), 1);
    // Confirm the items
    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    let (transferred_obj_ref, _) = order_effects.mutated[0];
    assert_ne!(gas_object_ref, transferred_obj_ref);

    assert_eq!(transferred_obj_ref.0, new_obj_ref.0);

    let transferred_obj_info = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    // Confirm new owner
    assert!(transferred_obj_info
        .object()
        .unwrap()
        .owner
        .is_address(&client2.address()));
}

#[tokio::test]
async fn test_move_calls_chain_many_authority_syncronization() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.clone().values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (mut last_certificate, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;

    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    // Get the object created from the call
    let (new_obj_ref, _) = order_effects.created[0];

    for value in 0u64..10u64 {
        // Fetch the full object
        let new_obj = client1
            .get_object_info(ObjectInfoRequest {
                object_id: new_obj_ref.0,
                request_sequence_number: None,
            })
            .await
            .unwrap();

        gas_object_ref = client1
            .get_object_info(ObjectInfoRequest {
                object_id: gas_object_ref.0,
                request_sequence_number: None,
            })
            .await
            .unwrap()
            .object()
            .unwrap()
            .to_object_reference();

        let pure_args = vec![bcs::to_bytes(&value).unwrap()];
        let _call_response = client1
            .move_call(
                framework_obj_ref,
                ident_str!("ObjectBasics").to_owned(),
                ident_str!("set_value").to_owned(),
                Vec::new(),
                gas_object_ref,
                vec![new_obj.object().unwrap().to_object_reference()],
                pure_args,
                GAS_VALUE_FOR_TESTING / 2,
            )
            .await;

        last_certificate = _call_response.unwrap().0;
    }

    // For this test to work the client has updated the first 3 authorities but not the last one
    // Assert this to catch any changes to the client behaviour that reqire fixing this test to still
    // test sync.

    let authorities: Vec<_> = authority_clients.clone().into_iter().collect();

    let full_seq = authorities[2]
        .1
        .handle_object_info_request(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();
    assert_eq!(full_seq.1, SequenceNumber::from(11));

    let zero_seq = authorities[3]
        .1
        .handle_object_info_request(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();
    assert_eq!(zero_seq.1, SequenceNumber::from(0));

    // This is (finally) the function we want to test

    // If we try to sync from the authority that does not have the data to the one
    // that does not we fail.
    let result = client1
        .authorities()
        .sync_authority_source_to_destination(
            ConfirmationOrder::new(last_certificate.clone()),
            authorities[3].0,
            authorities[3].0,
        )
        .await;

    assert!(result.is_err());

    // Here we get the list of objects known by authorities.
    let (obj_map, _auths) = client1
        .authorities()
        .get_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await
        .unwrap();
    // Check only 3 out of 4 authorities have the latest object
    assert_eq!(obj_map[&full_seq].len(), 3);

    // If we try to sync from the authority that does have the data to the one
    // that does not we succeed.
    let result = client1
        .authorities()
        .sync_authority_source_to_destination(
            ConfirmationOrder::new(last_certificate),
            authorities[2].0,
            authorities[3].0,
        )
        .await;

    // Here we get the list of objects known by authorities.
    let (obj_map, _auths) = client1
        .authorities()
        .get_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await
        .unwrap();
    // Check all 4 out of 4 authorities have the latest object
    assert_eq!(obj_map[&full_seq].len(), 4);

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_move_calls_chain_many_delete_authority_synchronization() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.clone().values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;

    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    // Get the object created from the call
    let (new_obj_ref, _) = order_effects.created[0];

    for value in 0u64..20u64 {
        // Fetch the full object
        let new_obj = client1
            .get_object_info(ObjectInfoRequest {
                object_id: new_obj_ref.0,
                request_sequence_number: None,
            })
            .await
            .unwrap();

        gas_object_ref = client1
            .get_object_info(ObjectInfoRequest {
                object_id: gas_object_ref.0,
                request_sequence_number: None,
            })
            .await
            .unwrap()
            .object()
            .unwrap()
            .to_object_reference();

        let pure_args = vec![bcs::to_bytes(&value).unwrap()];
        let _call_response = client1
            .move_call(
                framework_obj_ref,
                ident_str!("ObjectBasics").to_owned(),
                ident_str!("set_value").to_owned(),
                Vec::new(),
                gas_object_ref,
                vec![new_obj.object().unwrap().to_object_reference()],
                pure_args,
                GAS_VALUE_FOR_TESTING / 2,
            )
            .await;
    }

    // Fetch the full object
    let new_obj_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    gas_object_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("delete").to_owned(),
            Vec::new(),
            gas_object_ref,
            vec![new_obj_ref],
            Vec::new(),
            GAS_VALUE_FOR_TESTING / 2,
        )
        .await;

    let last_certificate = call_response.unwrap().0;

    // For this test to work the client has updated the first 3 authorities but not the last one
    // Assert this to catch any changes to the client behaviour that reqire fixing this test to still
    // test sync.

    let authorities: Vec<_> = authority_clients.clone().into_iter().collect();

    let full_seq = authorities[2]
        .1
        .handle_object_info_request(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();
    assert_eq!(full_seq.1, SequenceNumber::from(22));

    let zero_seq = authorities[3]
        .1
        .handle_object_info_request(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();
    assert_eq!(zero_seq.1, SequenceNumber::from(0));

    // This is (finally) the function we want to test

    // If we try to sync from the authority that does not have the data to the one
    // that does not have the data we fail.
    let result = client1
        .authorities()
        .sync_authority_source_to_destination(
            ConfirmationOrder::new(last_certificate.clone()),
            authorities[3].0,
            authorities[3].0,
        )
        .await;

    assert!(result.is_err());

    // If we try to sync from the authority that does have the data to the one
    // that does not we succeed.
    let result = client1
        .authorities()
        .sync_authority_source_to_destination(
            ConfirmationOrder::new(last_certificate),
            authorities[2].0,
            authorities[3].0,
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_move_calls_chain_many_delete_authority_auto_synchronization() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.clone().values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;
    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    // Get the object created from the call
    let (new_obj_ref, _) = order_effects.created[0];

    for value in 0u64..20u64 {
        // Fetch the full object
        let new_obj = client1
            .get_object_info(ObjectInfoRequest {
                object_id: new_obj_ref.0,
                request_sequence_number: None,
            })
            .await
            .unwrap();

        gas_object_ref = client1
            .get_object_info(ObjectInfoRequest {
                object_id: gas_object_ref.0,
                request_sequence_number: None,
            })
            .await
            .unwrap()
            .object()
            .unwrap()
            .to_object_reference();

        let pure_args = vec![bcs::to_bytes(&value).unwrap()];
        let _call_response = client1
            .move_call(
                framework_obj_ref,
                ident_str!("ObjectBasics").to_owned(),
                ident_str!("set_value").to_owned(),
                Vec::new(),
                gas_object_ref,
                vec![new_obj.object().unwrap().to_object_reference()],
                pure_args,
                GAS_VALUE_FOR_TESTING / 2,
            )
            .await;
    }

    // Fetch the full object
    let new_obj_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    gas_object_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("delete").to_owned(),
            Vec::new(),
            gas_object_ref,
            vec![new_obj_ref],
            Vec::new(),
            GAS_VALUE_FOR_TESTING / 2,
        )
        .await;

    let last_certificate = call_response.unwrap().0;

    // For this test to work the client has updated the first 3 authorities but not the last one
    // Assert this to catch any changes to the client behaviour that reqire fixing this test to still
    // test sync.

    let authorities: Vec<_> = authority_clients.clone().into_iter().collect();

    let full_seq = authorities[2]
        .1
        .handle_object_info_request(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();
    assert_eq!(full_seq.1, SequenceNumber::from(22));

    let zero_seq = authorities[3]
        .1
        .handle_object_info_request(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();
    assert_eq!(zero_seq.1, SequenceNumber::from(0));

    // This is (finally) the function we want to test

    // If we try to sync we succeed.
    let result = client1
        .authorities()
        .sync_certificate_to_authority_with_timeout(
            ConfirmationOrder::new(last_certificate),
            authorities[3].0,
            Duration::from_millis(1000), // ms
            2,                           // retry
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_move_calls_object_transfer_and_freeze() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let client2 = make_client(authority_clients.clone(), committee);

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;
    // Get the object created from the call
    let (new_obj_ref, _) = order_effects.created[0];
    // Fetch the full object
    let new_obj = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    gas_object_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    let pure_args = vec![bcs::to_bytes(&client2.address().to_vec()).unwrap()];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("transfer_and_freeze").to_owned(),
            Vec::new(),
            gas_object_ref,
            vec![new_obj.object().unwrap().to_object_reference()],
            pure_args,
            GAS_VALUE_FOR_TESTING / 2,
        )
        .await;

    // Check effects are good
    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;
    // Status flag should be success
    assert_eq!(order_effects.status, ExecutionStatus::Success);
    // Nothing should be deleted during a transfer
    assert!(order_effects.deleted.is_empty());
    // Item being transfered is mutated.
    assert_eq!(order_effects.mutated.len(), 1);

    let (transferred_obj_ref, _) = order_effects.mutated[0];
    assert_ne!(gas_object_ref, transferred_obj_ref);

    assert_eq!(transferred_obj_ref.0, new_obj_ref.0);

    let transferred_obj_info = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    // Confirm new owner
    assert!(transferred_obj_info
        .object()
        .unwrap()
        .owner
        .is_address(&client2.address()));

    // Confirm read only
    assert!(transferred_obj_info.object().unwrap().is_read_only());
}

#[tokio::test]
async fn test_move_calls_object_delete() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee);

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (_, order_info_resp) = call_response.unwrap();
    // Get the object created from the call
    let order_effects = order_info_resp.signed_effects.unwrap().effects;
    let (new_obj_ref, _) = order_effects.created[0];
    // Fetch the full object
    let new_obj = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    gas_object_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    let call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("delete").to_owned(),
            Vec::new(),
            gas_object_ref,
            vec![new_obj.object().unwrap().to_object_reference()],
            Vec::new(),
            GAS_VALUE_FOR_TESTING / 2,
        )
        .await;

    // Check effects are good
    let (_, order_info_resp) = call_response.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;

    // Status flag should be success
    assert_eq!(order_effects.status, ExecutionStatus::Success);
    // Object be deleted during a delete
    assert_eq!(order_effects.deleted.len(), 1);
    // No item is mutated.
    assert_eq!(order_effects.mutated.len(), 0);
    // Confirm the items
    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    // Try to fetch the deleted object
    let deleted_object_resp = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    assert!(deleted_object_resp.object_and_lock.is_none());
}

#[tokio::test]
async fn test_move_calls_certs() {
    let (authority_clients, committee) = init_local_authorities(1).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let mut client2 = make_client(authority_clients.clone(), committee);

    let gas_object_id = ObjectID::random();

    let framework_obj_ref = client1
        .get_object_info(ObjectInfoRequest {
            object_id: FASTX_FRAMEWORK_ADDRESS,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .to_object_reference();

    // Populate authorities with obj data
    let authority_objects = vec![vec![gas_object_id]];

    let gas_object_ref = fund_account(
        authority_clients.values().collect(),
        &mut client1,
        authority_objects,
    )
    .await
    .get(&gas_object_id)
    .unwrap()
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let object_value: u64 = 100;
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];

    // Create new object with move
    let (cert, order_info_resp) = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await
        .unwrap();
    let effect = order_info_resp.signed_effects.unwrap().effects;
    let new_object_ref = &effect.created[0].0;

    let gas_object_ref = &effect.gas_object.0;

    let (new_object_id, _, _) = &new_object_ref;

    // Client 1 should have one certificate, one new object and one gas object, each with one associated certificate.
    assert!(client1
        .store()
        .certificates
        .contains_key(&cert.order.digest())
        .unwrap());
    assert_eq!(1, client1.store().certificates.iter().count());
    assert_eq!(2, client1.store().object_sequence_numbers.iter().count());
    assert_eq!(2, client1.store().object_certs.iter().count());
    assert!(client1
        .store()
        .object_certs
        .contains_key(&gas_object_id)
        .unwrap());
    assert!(client1
        .store()
        .object_certs
        .contains_key(new_object_id)
        .unwrap());
    assert_eq!(
        1,
        client1
            .store()
            .object_certs
            .get(&gas_object_id)
            .unwrap()
            .unwrap()
            .len()
    );
    assert_eq!(
        1,
        client1
            .store()
            .object_certs
            .get(new_object_id)
            .unwrap()
            .unwrap()
            .len()
    );
    assert_eq!(
        OBJECT_START_VERSION,
        client1
            .store()
            .object_sequence_numbers
            .get(&gas_object_id)
            .unwrap()
            .unwrap()
            .clone()
    );
    assert_eq!(
        OBJECT_START_VERSION,
        client1
            .store()
            .object_sequence_numbers
            .get(new_object_id)
            .unwrap()
            .unwrap()
            .clone()
    );

    // Transfer object with move
    let pure_args = vec![bcs::to_bytes(&client2.address().to_vec()).unwrap()];
    let (cert, _) = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("transfer").to_owned(),
            Vec::new(),
            *gas_object_ref,
            vec![*new_object_ref],
            pure_args,
            GAS_VALUE_FOR_TESTING / 2, // Make sure budget is less than gas value
        )
        .await
        .unwrap();

    // Client 1 should have two certificate, one gas object, with two associated certificate.
    assert!(client1
        .store()
        .certificates
        .contains_key(&cert.order.digest())
        .unwrap());
    assert_eq!(client1.store().certificates.iter().count(), 2);
    assert_eq!(client1.store().object_sequence_numbers.iter().count(), 1);
    assert_eq!(client1.store().object_certs.iter().count(), 1);
    assert!(client1
        .store()
        .object_certs
        .contains_key(&gas_object_id)
        .unwrap());
    assert_eq!(
        2,
        client1
            .store()
            .object_certs
            .get(&gas_object_id)
            .unwrap()
            .unwrap()
            .len()
    );
    assert_eq!(
        SequenceNumber::from(2),
        client1
            .store()
            .object_sequence_numbers
            .get(&gas_object_id)
            .unwrap()
            .unwrap()
            .clone()
    );

    // Sync client 2
    client2.sync_client_state().await.unwrap();

    // Client 2 should have 2 certificate, one new object, with two associated certificate.
    assert_eq!(2, client2.store().certificates.iter().count());
    assert_eq!(1, client2.store().object_sequence_numbers.iter().count());
    assert_eq!(1, client2.store().object_certs.iter().count());
    assert!(client2
        .store()
        .object_certs
        .contains_key(new_object_id)
        .unwrap());
    assert_eq!(
        2,
        client2
            .store()
            .object_certs
            .get(new_object_id)
            .unwrap()
            .unwrap()
            .len()
    );
    assert_eq!(
        SequenceNumber::from(2),
        client2
            .store()
            .object_sequence_numbers
            .get(new_object_id)
            .unwrap()
            .unwrap()
            .clone()
    );
}

#[tokio::test]
async fn test_module_publish_and_call_good() {
    // Init the states
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee);

    let gas_object_id = ObjectID::random();

    // Populate authorities with gas obj data
    let gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // Provide path to well formed package sources
    let mut hero_path = env!("CARGO_MANIFEST_DIR").to_owned();
    hero_path.push_str("/../fastx_programmability/examples/");

    let pub_res = client1.publish(hero_path, gas_object_ref).await;

    let (_, order_info_resp) = pub_res.unwrap();
    let published_effects = order_info_resp.signed_effects.unwrap().effects;

    // Only package obj should be created
    assert_eq!(published_effects.created.len(), 1);

    // Verif gas obj
    assert_eq!(published_effects.gas_object.0 .0, gas_object_ref.0);

    let (new_obj_ref, _) = published_effects.created.get(0).unwrap();
    assert_ne!(gas_object_ref, *new_obj_ref);

    // We now have the module obj ref
    // We can inspect it

    let new_obj = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    // Version should be 1 for all modules
    assert_eq!(new_obj.object().unwrap().version(), OBJECT_START_VERSION);
    // Must be immutable
    assert!(new_obj.object().unwrap().is_read_only());

    // StructTag type is not defined for package
    assert!(new_obj.object().unwrap().type_().is_none());

    // Data should be castable as a package
    assert!(new_obj.object().unwrap().data.try_as_package().is_some());

    // Retrieve latest gas obj spec
    let gas_object = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_id,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .clone();

    let gas_object_ref = gas_object.to_object_reference();

    //Try to call a function in TrustedCoin module
    let call_resp = client1
        .move_call(
            new_obj.object().unwrap().to_object_reference(),
            ident_str!("TrustedCoin").to_owned(),
            ident_str!("init").to_owned(),
            vec![],
            gas_object_ref,
            vec![],
            vec![],
            1000,
        )
        .await
        .unwrap();

    let effects = call_resp.1.signed_effects.unwrap().effects;
    assert!(effects.status == ExecutionStatus::Success);

    // This gets the treasury cap for the coin and gives it to the sender
    let tres_cap_ref = effects
        .created
        .iter()
        .find(|r| r.0 .0 != gas_object_ref.0)
        .unwrap()
        .0;

    // Fetch the full obj info
    let tres_cap_obj_info = client1
        .get_object_info(ObjectInfoRequest {
            object_id: tres_cap_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();
    // Confirm we own this object
    assert_eq!(tres_cap_obj_info.object().unwrap().owner, gas_object.owner);
}

// Pass a file in a package dir instead of the root. The builder should be able to infer the root
#[tokio::test]
async fn test_module_publish_file_path() {
    // Init the states
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee);

    let gas_object_id = ObjectID::random();

    // Populate authorities with gas obj data
    let gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // Compile
    let mut hero_path = env!("CARGO_MANIFEST_DIR").to_owned();

    // Use a path pointing to a different file
    hero_path.push_str("/../fastx_programmability/examples/Hero.move");

    let pub_resp = client1.publish(hero_path, gas_object_ref).await;

    let (_, order_info_resp) = pub_resp.unwrap();
    let published_effects = order_info_resp.signed_effects.unwrap().effects;

    // Only package obj should be created
    assert_eq!(published_effects.created.len(), 1);

    // Verif gas
    assert_eq!(published_effects.gas_object.0 .0, gas_object_ref.0);

    let (new_obj_ref, _) = published_effects.created.get(0).unwrap();
    assert_ne!(gas_object_ref, *new_obj_ref);

    // We now have the module obj ref
    // We can inspect it
    let new_obj = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    // Version should be 1 for all modules
    assert_eq!(new_obj.object().unwrap().version(), OBJECT_START_VERSION);
    // Must be immutable
    assert!(new_obj.object().unwrap().is_read_only());

    // StructTag type is not defined for package
    assert!(new_obj.object().unwrap().type_().is_none());

    // Data should be castable as a package
    assert!(new_obj.object().unwrap().data.try_as_package().is_some());

    // Retrieve latest gas obj spec
    let gas_object = client1
        .get_object_info(ObjectInfoRequest {
            object_id: gas_object_id,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object()
        .unwrap()
        .clone();

    let gas_object_ref = gas_object.to_object_reference();

    // Even though we provided a path to Hero.move, the builder is able to find the package root
    // build all in the package, including TrustedCoin module
    //Try to call a function in TrustedCoin module
    let call_resp = client1
        .move_call(
            new_obj.object().unwrap().to_object_reference(),
            ident_str!("TrustedCoin").to_owned(),
            ident_str!("init").to_owned(),
            vec![],
            gas_object_ref,
            vec![],
            vec![],
            1000,
        )
        .await;
    assert!(call_resp.is_ok());
}

#[tokio::test]
async fn test_module_publish_bad_path() {
    // Init the states
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee);

    let gas_object_id = ObjectID::random();

    // Populate authorities with gas obj data
    let gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // Compile
    let mut hero_path = env!("CARGO_MANIFEST_DIR").to_owned();

    // Use a bad path
    hero_path.push_str("/../fastx_____programmability/examples/");

    let pub_resp = client1.publish(hero_path, gas_object_ref).await;
    // Has to fail
    assert!(pub_resp.is_err());
}

#[tokio::test]
async fn test_module_publish_naughty_path() {
    // Init the states
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee);

    let gas_object_id = ObjectID::random();

    // Populate authorities with gas obj data
    let gas_object_ref = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    for ns in naughty_strings::BLNS {
        // Compile
        let mut hero_path = env!("CARGO_MANIFEST_DIR").to_owned();

        // Use a bad path
        hero_path.push_str(&format!("/../{}", ns));

        let pub_resp = client1.publish(hero_path, gas_object_ref).await;
        // Has to fail
        assert!(pub_resp.is_err());
    }
}

#[test]
fn test_transfer_object_error() {
    let rt = Runtime::new().unwrap();
    let (recipient, _) = get_key_pair();

    let objects: Vec<ObjectID> = (0..10).map(|_| ObjectID::random()).collect();
    let gas_object = ObjectID::random();
    let number_of_authorities = 4;

    let mut all_objects = objects.clone();
    all_objects.push(gas_object);
    let authority_objects = (0..number_of_authorities)
        .map(|_| all_objects.clone())
        .collect();

    let mut sender = rt.block_on(init_local_client_state(authority_objects));

    let mut objects = objects.iter();

    // Test 1: Double spend
    let object_id = *objects.next().unwrap();
    rt.block_on(sender.transfer_object(object_id, gas_object, recipient))
        .unwrap();
    let result = rt.block_on(sender.transfer_object(object_id, gas_object, recipient));

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(FastPayError::ObjectNotFound { .. })
    ));

    // Test 2: Object not known to authorities
    let obj = Object::with_id_owner_for_testing(ObjectID::random(), sender.address());
    sender
        .store()
        .object_refs
        .insert(&obj.id(), &obj.to_object_reference())
        .unwrap();
    sender
        .store()
        .object_sequence_numbers
        .insert(&obj.id(), &SequenceNumber::new())
        .unwrap();
    let result = rt.block_on(sender.transfer_object(obj.id(), gas_object, recipient));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err().downcast_ref(),
            Some(FastPayError::QuorumNotReached {errors, ..}) if matches!(errors.as_slice(), [FastPayError::ObjectNotFound{..}, ..])));

    // Test 3: invalid object digest
    let object_id = *objects.next().unwrap();

    // give object an incorrect object digest
    sender
        .store()
        .object_refs
        .insert(
            &object_id,
            &(object_id, SequenceNumber::new(), ObjectDigest([0; 32])),
        )
        .unwrap();

    let result = rt.block_on(sender.transfer_object(object_id, gas_object, recipient));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err().downcast_ref(),
            Some(FastPayError::QuorumNotReached {errors, ..}) if matches!(errors.as_slice(), [FastPayError::LockErrors{..}, ..])));

    // Test 4: Invalid sequence number;
    let object_id = *objects.next().unwrap();

    // give object an incorrect sequence number
    sender
        .store()
        .object_sequence_numbers
        .insert(&object_id, &SequenceNumber::from(2))
        .unwrap();

    let result = rt.block_on(sender.transfer_object(object_id, gas_object, recipient));
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(FastPayError::UnexpectedSequenceNumber { .. })
    ));

    // Test 5: The client does not allow concurrent transfer;
    let object_id = *objects.next().unwrap();
    // Fabricate a fake pending transfer
    let transfer = Transfer {
        sender: sender.address(),
        recipient: FastPayAddress::random_for_testing_only(),
        object_ref: (object_id, Default::default(), ObjectDigest::new([0; 32])),
        gas_payment: (gas_object, Default::default(), ObjectDigest::new([0; 32])),
    };
    sender
        .lock_pending_order_objects(&Order::new(
            OrderKind::Transfer(transfer),
            &get_key_pair().1,
        ))
        .unwrap();

    let result = rt.block_on(sender.transfer_object(object_id, gas_object, recipient));
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(FastPayError::ConcurrentTransactionError)
    ))
}

#[tokio::test]
async fn test_receive_object_error() -> Result<(), anyhow::Error> {
    let number_of_authorities = 4;
    let (authority_clients, committee) = init_local_authorities(number_of_authorities).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let mut client2 = make_client(authority_clients.clone(), committee);

    let objects: Vec<ObjectID> = (0..10).map(|_| ObjectID::random()).collect();
    let gas_object = ObjectID::random();
    let gas_object_2 = ObjectID::random();
    let mut all_objects = objects.clone();
    all_objects.push(gas_object);
    fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        all_objects,
    )
    .await;
    fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client2,
        vec![gas_object_2],
    )
    .await;

    let mut objects = objects.iter();
    // Test 1: Recipient is not us.
    let object_id = *objects.next().unwrap();
    let (certificate, _) = client1
        .transfer_object(
            object_id,
            gas_object,
            FastPayAddress::random_for_testing_only(),
        )
        .await?;

    let result = client2.receive_object(&certificate).await;

    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(FastPayError::IncorrectRecipientError)
    ));

    // Test 2: Receive tempered certificate order.
    let (transfer, sig) = match certificate.order {
        Order {
            kind: OrderKind::Transfer(t),
            signature,
        } => Some((t, signature)),
        _ => None,
    }
    .unwrap();

    let malformed_order = CertifiedOrder {
        order: Order {
            kind: OrderKind::Transfer(Transfer {
                sender: client1.address(),
                recipient: client2.address(),
                object_ref: transfer.object_ref,
                gas_payment: transfer.gas_payment,
            }),
            signature: sig,
        },
        signatures: certificate.signatures,
    };

    let result = client2.receive_object(&malformed_order).await;
    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(FastPayError::InvalidSignature { .. })
    ));

    Ok(())
}

#[test]
fn test_client_store() {
    let store =
        ClientStore::new(env::temp_dir().join(format!("CLIENT_DB_{:?}", ObjectID::random())));

    // Make random sequence numbers
    let keys_vals = (0..100)
        .map(|i| (ObjectID::random(), SequenceNumber::from(i)))
        .collect::<Vec<_>>();
    // Try insert batch
    store
        .object_sequence_numbers
        .multi_insert(keys_vals.clone().into_iter())
        .unwrap();

    // Check the size
    assert_eq!(store.object_sequence_numbers.iter().count(), 100);

    // Check that the items are all correct
    keys_vals.iter().for_each(|(k, v)| {
        assert_eq!(*v, store.object_sequence_numbers.get(k).unwrap().unwrap());
    });

    // Check that are removed
    store
        .object_sequence_numbers
        .multi_remove(keys_vals.into_iter().map(|(k, _)| k))
        .unwrap();

    assert!(store.object_sequence_numbers.is_empty());
}

#[tokio::test]
async fn test_object_store() {
    // Init the states
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());

    let gas_object_id = ObjectID::random();

    // Populate authorities with gas obj data
    let gas_object = fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .clone();
    let gas_object_ref = gas_object.clone().to_object_reference();
    // Ensure that object store is empty
    assert!(client1.store().objects.is_empty());

    // Run a few syncs to retrieve objects ids
    for _ in 0..4 {
        let _ = client1.sync_client_state().await.unwrap();
    }
    // Try to download objects which are not already in storage
    client1.download_owned_objects_not_in_db().await.unwrap();

    // Gas object should be in storage now
    assert_eq!(client1.store().objects.iter().count(), 1);

    // Verify that we indeed have the object
    let gas_obj_from_store = client1
        .store()
        .objects
        .get(&gas_object_ref)
        .unwrap()
        .unwrap();
    assert_eq!(gas_obj_from_store, gas_object);

    // Provide path to well formed package sources
    let mut hero_path = env!("CARGO_MANIFEST_DIR").to_owned();
    hero_path.push_str("/../fastx_programmability/examples/");

    let pub_res = client1.publish(hero_path, gas_object_ref).await;

    let (_, order_info_resp) = pub_res.unwrap();
    let published_effects = order_info_resp.signed_effects.unwrap().effects;

    // Only package obj should be created
    assert_eq!(published_effects.created.len(), 1);

    // Verif gas obj
    assert_eq!(published_effects.gas_object.0 .0, gas_object_ref.0);

    let (new_obj_ref, _) = published_effects.created.get(0).unwrap();
    assert_ne!(gas_object_ref, *new_obj_ref);

    // We now have the module obj ref
    // We can inspect it
    let new_obj = client1
        .get_object_info(ObjectInfoRequest {
            object_id: new_obj_ref.0,
            request_sequence_number: None,
        })
        .await
        .unwrap();

    // Published object should be in storage now
    // But also the new gas object should be in storage, so 2 new items, plus 1 from before
    assert_eq!(client1.store().objects.iter().count(), 3);

    // Verify that we indeed have the new module object
    let mod_obj_from_store = client1.store().objects.get(new_obj_ref).unwrap().unwrap();
    assert_eq!(mod_obj_from_store, *new_obj.object().unwrap());
}

#[tokio::test]
async fn test_object_store_transfer() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());
    let mut client2 = make_client(authority_clients.clone(), committee);

    let object_id = ObjectID::random();
    let gas_object1 = ObjectID::random();
    let gas_object2 = ObjectID::random();

    fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client1,
        vec![object_id, gas_object1],
    )
    .await;
    fund_account_with_same_objects(
        authority_clients.values().collect(),
        &mut client2,
        vec![gas_object2],
    )
    .await;

    // Clients should not have retrieved objects
    assert_eq!(client1.store().objects.iter().count(), 0);
    assert_eq!(client2.store().objects.iter().count(), 0);

    // Run a few syncs to populate object ids
    for _ in 0..4 {
        let _ = client1.sync_client_state().await.unwrap();
        let _ = client2.sync_client_state().await.unwrap();
    }

    // Try to download objects which are not already in storage
    client1.download_owned_objects_not_in_db().await.unwrap();
    client2.download_owned_objects_not_in_db().await.unwrap();

    // Gas object and another object should be in storage now for client 1
    assert_eq!(client1.store().objects.iter().count(), 2);

    // Only gas object should be in storage now for client 2
    assert_eq!(client2.store().objects.iter().count(), 1);

    // Transfer object to client2.
    let (certificate, _) = client1
        .transfer_object(object_id, gas_object1, client2.address())
        .await
        .unwrap();

    // Update client2's local object data.
    client2.receive_object(&certificate).await.unwrap();

    // Client 1 should not have lost its objects
    // Plus it should have a new gas object
    assert_eq!(client1.store().objects.iter().count(), 3);
    // Client 2 should now have the new object
    assert_eq!(client2.store().objects.iter().count(), 2);

    // Transfer the object back to Client1
    let (certificate, _) = client2
        .transfer_object(object_id, gas_object2, client1.address())
        .await
        .unwrap();
    // Update client1's local object data.
    client1.receive_object(&certificate).await.unwrap();

    // Client 1 should have a new version of the object back
    assert_eq!(client1.store().objects.iter().count(), 4);
    // Client 2 should have new gas object version
    assert_eq!(client2.store().objects.iter().count(), 3);
}

#[tokio::test]
async fn test_transfer_pending_orders() {
    let objects: Vec<ObjectID> = (0..15).map(|_| ObjectID::random()).collect();
    let gas_object = ObjectID::random();
    let number_of_authorities = 4;

    let mut all_objects = objects.clone();
    all_objects.push(gas_object);
    let authority_objects = (0..number_of_authorities)
        .map(|_| all_objects.clone())
        .collect();

    let mut sender_state = init_local_client_state(authority_objects).await;
    let recipient = init_local_client_state(vec![vec![]]).await.address();

    let mut objects = objects.iter();

    // Test 1: Normal transfer
    let object_id = *objects.next().unwrap();
    sender_state
        .transfer_object(object_id, gas_object, recipient)
        .await
        .unwrap();
    // Pending order should be cleared
    assert!(sender_state.store().pending_orders.is_empty());

    // Test 2: Object not known to authorities. This has no side effect
    let obj = Object::with_id_owner_for_testing(ObjectID::random(), sender_state.address());
    sender_state
        .store()
        .object_refs
        .insert(&obj.id(), &obj.to_object_reference())
        .unwrap();
    sender_state
        .store()
        .object_sequence_numbers
        .insert(&obj.id(), &SequenceNumber::new())
        .unwrap();
    let result = sender_state
        .transfer_object(obj.id(), gas_object, recipient)
        .await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err().downcast_ref(),
            Some(FastPayError::QuorumNotReached {errors, ..}) if matches!(errors.as_slice(), [FastPayError::ObjectNotFound{..}, ..])));
    // Pending order should be cleared
    assert!(sender_state.store().pending_orders.is_empty());

    // Test 3: invalid object digest. This also has no side effect
    let object_id = *objects.next().unwrap();

    // give object an incorrect object digest
    sender_state
        .store()
        .object_refs
        .insert(
            &object_id,
            &(object_id, SequenceNumber::new(), ObjectDigest([0; 32])),
        )
        .unwrap();

    let result = sender_state
        .transfer_object(object_id, gas_object, recipient)
        .await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err().downcast_ref(),
            Some(FastPayError::QuorumNotReached {errors, ..}) if matches!(errors.as_slice(), [FastPayError::LockErrors{..}, ..])));

    // Pending order should be cleared
    assert!(sender_state.store().pending_orders.is_empty());

    // Test 4: Conflicting orders touching same objects
    let object_id = *objects.next().unwrap();
    // Fabricate a fake pending transfer
    let transfer = Transfer {
        sender: sender_state.address(),
        recipient: FastPayAddress::random_for_testing_only(),
        object_ref: (object_id, Default::default(), ObjectDigest::new([0; 32])),
        gas_payment: (gas_object, Default::default(), ObjectDigest::new([0; 32])),
    };
    // Simulate locking some objects
    sender_state
        .lock_pending_order_objects(&Order::new(
            OrderKind::Transfer(transfer),
            &get_key_pair().1,
        ))
        .unwrap();
    // Try to use those objects in another order
    let result = sender_state
        .transfer_object(object_id, gas_object, recipient)
        .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(FastPayError::ConcurrentTransactionError)
    ));
    // clear the pending orders
    sender_state.store().pending_orders.clear().unwrap();
    assert_eq!(sender_state.store().pending_orders.iter().count(), 0);
}

#[tokio::test]
async fn test_full_client_sync_move_calls() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.clone().values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_res = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;

    let (mut last_certificate, order_info_resp) = call_res.unwrap();
    let call_effects = order_info_resp.signed_effects.unwrap().effects;

    assert_eq!(call_effects.gas_object.0 .0, gas_object_id);

    // Get the object created from the call
    let (new_obj_ref, _) = call_effects.created[0];

    for value in 0u64..10u64 {
        // Fetch the full object
        let new_obj_ref = client_object(&mut client1, new_obj_ref.0).await.0;
        gas_object_ref = client_object(&mut client1, gas_object_id).await.0;

        let pure_args = vec![bcs::to_bytes(&value).unwrap()];
        let _call_response = client1
            .move_call(
                framework_obj_ref,
                ident_str!("ObjectBasics").to_owned(),
                ident_str!("set_value").to_owned(),
                Vec::new(),
                gas_object_ref,
                vec![new_obj_ref],
                pure_args,
                GAS_VALUE_FOR_TESTING / 2,
            )
            .await;

        last_certificate = _call_response.unwrap().0;
    }

    // For this test to work the client has updated the first 3 authorities but not the last one
    // Assert this to catch any changes to the client behaviour that reqire fixing this test to still
    // test sync.

    let authorities: Vec<_> = authority_clients.clone().into_iter().collect();

    let (full_seq, _) = auth_object(&authorities[2].1, gas_object_id).await;
    assert_eq!(full_seq.1, SequenceNumber::from(11));

    let (zero_seq, _) = auth_object(&authorities[3].1, gas_object_id).await;
    assert_eq!(zero_seq.1, SequenceNumber::from(0));

    // This is (finally) the function we want to test

    // If we try to sync from the authority that does not have the data to the one
    // that does not we fail.
    let result = client1
        .authorities()
        .sync_authority_source_to_destination(
            ConfirmationOrder::new(last_certificate.clone()),
            authorities[3].0,
            authorities[3].0,
        )
        .await;

    assert!(result.is_err());

    // Here we get the list of objects known by authorities.
    let (obj_map, _auths) = client1
        .authorities()
        .get_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await
        .unwrap();
    // Check only 3 out of 4 authorities have the latest object
    assert_eq!(obj_map[&full_seq].len(), 3);

    // We sync all the client objects
    let result = client1
        .authorities()
        .sync_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await;

    let (active, deleted) = result.unwrap();

    assert_eq!(0, deleted.len());
    assert_eq!(2, active.len());

    // Here we get the list of objects known by authorities.
    let (obj_map, _auths) = client1
        .authorities()
        .get_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await
        .unwrap();
    // Check all 4 out of 4 authorities have the latest object
    assert_eq!(obj_map[&full_seq].len(), 4);
}

#[tokio::test]
async fn test_full_client_sync_delete_calls() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let mut client1 = make_client(authority_clients.clone(), committee.clone());

    let object_value: u64 = 100;
    let gas_object_id = ObjectID::random();
    let framework_obj_ref = client1.get_framework_object_ref().await.unwrap();

    // Populate authorities with obj data
    let mut gas_object_ref = fund_account_with_same_objects(
        authority_clients.clone().values().collect(),
        &mut client1,
        vec![gas_object_id],
    )
    .await
    .iter()
    .next()
    .unwrap()
    .1
    .to_object_reference();

    let gas_id = gas_object_ref.0;

    // When creating an ObjectBasics object, we provide the value (u64) and address which will own the object
    let pure_args = vec![
        object_value.to_le_bytes().to_vec(),
        bcs::to_bytes(&client1.address().to_vec()).unwrap(),
    ];
    let call_res = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("create").to_owned(),
            Vec::new(),
            gas_object_ref,
            Vec::new(),
            pure_args,
            GAS_VALUE_FOR_TESTING - 1, // Make sure budget is less than gas value
        )
        .await;
    let (_, order_info_resp) = call_res.unwrap();
    let order_effects = order_info_resp.signed_effects.unwrap().effects;

    assert_eq!(order_effects.gas_object.0 .0, gas_object_id);

    // Get the object created from the call
    let (new_obj_ref, _) = order_effects.created[0];

    for value in 0u64..20u64 {
        // Fetch the full object
        let new_obj_ref = client_object(&mut client1, new_obj_ref.0).await.0;
        gas_object_ref = client_object(&mut client1, gas_id).await.0;

        let pure_args = vec![bcs::to_bytes(&value).unwrap()];
        let _call_response = client1
            .move_call(
                framework_obj_ref,
                ident_str!("ObjectBasics").to_owned(),
                ident_str!("set_value").to_owned(),
                Vec::new(),
                gas_object_ref,
                vec![new_obj_ref],
                pure_args,
                GAS_VALUE_FOR_TESTING / 2,
            )
            .await;
    }

    // Fetch the full object
    let new_obj_ref = client_object(&mut client1, new_obj_ref.0).await.0;
    gas_object_ref = client_object(&mut client1, gas_id).await.0;

    // We sync before we delete
    let result = client1
        .authorities()
        .sync_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await;

    let (active, deleted) = result.unwrap();
    assert_eq!(0, deleted.len());
    assert_eq!(2, active.len());

    let _call_response = client1
        .move_call(
            framework_obj_ref,
            ident_str!("ObjectBasics").to_owned(),
            ident_str!("delete").to_owned(),
            Vec::new(),
            gas_object_ref,
            vec![new_obj_ref],
            Vec::new(),
            GAS_VALUE_FOR_TESTING / 2,
        )
        .await;

    // For this test to work the client has updated the first 3 authorities but not the last one
    // Assert this to catch any changes to the client behaviour that reqire fixing this test to still
    // test sync.

    let authorities: Vec<_> = authority_clients.clone().into_iter().collect();

    let (full_seq, _) = auth_object(&authorities[2].1, gas_id).await;
    assert_eq!(full_seq.1, SequenceNumber::from(22));

    let (zero_seq, _) = auth_object(&authorities[3].1, gas_id).await;
    assert_eq!(zero_seq.1, SequenceNumber::from(21));

    // This is (finally) the function we want to test
    // Here we get the list of objects known by authorities.
    let (obj_map, _auths) = client1
        .authorities()
        .get_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await
        .unwrap();
    // Check only 3 out of 4 authorities have the latest object
    assert_eq!(obj_map[&full_seq].len(), 3);

    // We sync all the client objects
    let result = client1
        .authorities()
        .sync_all_owned_objects(client1.address(), Duration::from_secs(10))
        .await;

    let (active, deleted) = result.unwrap();

    assert_eq!(1, deleted.len());
    assert_eq!(1, active.len());
}

// A helper function to make tests less verbose
async fn client_object(client: &mut dyn Client, object_id: ObjectID) -> (ObjectRef, Object) {
    let object = client
        .get_object_info(ObjectInfoRequest {
            object_id,
            request_sequence_number: None,
        })
        .await
        .unwrap()
        .object_and_lock
        .unwrap()
        .object;

    (object.to_object_reference(), object)
}

// A helper function to make tests less verbose
async fn auth_object(authority: &LocalAuthorityClient, object_id: ObjectID) -> (ObjectRef, Object) {
    let response = authority
        .handle_object_info_request(ObjectInfoRequest::from(object_id))
        .await
        .unwrap();

    let object = response.object_and_lock.unwrap().object;
    (object.to_object_reference(), object)
}

#[tokio::test]
async fn test_map_reducer() {
    let (authority_clients, committee) = init_local_authorities(4).await;
    let client1 = make_client(authority_clients.clone(), committee.clone());

    // Test: reducer errors get propagated up
    let res = client1
        .authorities()
        .quorum_map_then_reduce_with_timeout(
            0usize,
            |_name, _client| Box::pin(async move { Ok(()) }),
            |_accumulated_state, _authority_name, _authority_weight, _result| {
                Box::pin(async move { Err(FastPayError::TooManyIncorrectAuthorities) })
            },
            Duration::from_millis(1000),
        )
        .await;
    assert!(Err(FastPayError::TooManyIncorrectAuthorities) == res);

    // Test: mapper errors do not get propagated up, reducer works
    let res = client1
        .authorities()
        .quorum_map_then_reduce_with_timeout(
            0usize,
            |_name, _client| {
                Box::pin(async move {
                    let res: Result<usize, FastPayError> =
                        Err(FastPayError::TooManyIncorrectAuthorities);
                    res
                })
            },
            |mut accumulated_state, _authority_name, _authority_weight, result| {
                Box::pin(async move {
                    assert!(Err(FastPayError::TooManyIncorrectAuthorities) == result);
                    accumulated_state += 1;
                    Ok(ReduceOutput::Continue(accumulated_state))
                })
            },
            Duration::from_millis(1000),
        )
        .await;
    assert_eq!(Ok(4), res);

    // Test: early end
    let res = client1
        .authorities()
        .quorum_map_then_reduce_with_timeout(
            0usize,
            |_name, _client| Box::pin(async move { Ok(()) }),
            |mut accumulated_state, _authority_name, _authority_weight, _result| {
                Box::pin(async move {
                    if accumulated_state > 2 {
                        Ok(ReduceOutput::End(accumulated_state))
                    } else {
                        accumulated_state += 1;
                        Ok(ReduceOutput::Continue(accumulated_state))
                    }
                })
            },
            Duration::from_millis(1000),
        )
        .await;
    assert_eq!(Ok(3), res);

    // Test: Global timeout works
    let res = client1
        .authorities()
        .quorum_map_then_reduce_with_timeout(
            0usize,
            |_name, _client| {
                Box::pin(async move {
                    // 10 mins
                    tokio::time::sleep(Duration::from_secs(10 * 60)).await;
                    Ok(())
                })
            },
            |_accumulated_state, _authority_name, _authority_weight, _result| {
                Box::pin(async move { Err(FastPayError::TooManyIncorrectAuthorities) })
            },
            Duration::from_millis(10),
        )
        .await;
    assert_eq!(Ok(0), res);

    // Test: Local timeout works
    let bad_auth = *committee.sample();
    let res = client1
        .authorities()
        .quorum_map_then_reduce_with_timeout(
            HashSet::new(),
            |_name, _client| {
                Box::pin(async move {
                    // 10 mins
                    if _name == bad_auth {
                        tokio::time::sleep(Duration::from_secs(10 * 60)).await;
                    }
                    Ok(())
                })
            },
            |mut accumulated_state, authority_name, _authority_weight, _result| {
                Box::pin(async move {
                    accumulated_state.insert(authority_name);
                    if accumulated_state.len() <= 3 {
                        Ok(ReduceOutput::Continue(accumulated_state))
                    } else {
                        Ok(ReduceOutput::ContinueWithTimeout(
                            accumulated_state,
                            Duration::from_millis(10),
                        ))
                    }
                })
            },
            // large delay
            Duration::from_millis(10 * 60),
        )
        .await;
    assert_eq!(res.as_ref().unwrap().len(), 3);
    assert!(!res.as_ref().unwrap().contains(&bad_auth));
}
