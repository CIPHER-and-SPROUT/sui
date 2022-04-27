// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![deny(warnings)]

use crate::benchmark::validator_preparer::ValidatorPreparer;
use bytes::Bytes;
use move_core_types::account_address::AccountAddress;
use move_core_types::ident_str;
use rayon::prelude::*;
use sui_types::crypto::{get_key_pair, AuthoritySignature, KeyPair, PublicKeyBytes, Signature};
use sui_types::SUI_FRAMEWORK_ADDRESS;
use sui_types::{base_types::*, committee::*, messages::*, object::Object, serialize::*};

const OBJECT_ID_OFFSET: &str = "0x10000";
const GAS_PER_TX: u64 = 10000000;

/// Create a transaction for object transfer
/// This can either use the Move path or the native path
fn make_transfer_transaction(
    object_ref: ObjectRef,
    recipient: SuiAddress,
    use_move: bool,
) -> SingleTransactionKind {
    if use_move {
        let framework_obj_ref = (
            ObjectID::from(SUI_FRAMEWORK_ADDRESS),
            SequenceNumber::new(),
            ObjectDigest::new([0; 32]),
        );

        SingleTransactionKind::Call(MoveCall {
            package: framework_obj_ref,
            module: ident_str!("SUI").to_owned(),
            function: ident_str!("transfer").to_owned(),
            type_arguments: Vec::new(),
            arguments: vec![
                CallArg::ImmOrOwnedObject(object_ref),
                CallArg::Pure(bcs::to_bytes(&AccountAddress::from(recipient)).unwrap()),
            ],
        })
    } else {
        SingleTransactionKind::Transfer(Transfer {
            recipient,
            object_ref,
        })
    }
}

/// Creates an object for use in the microbench
fn create_gas_object(object_id: ObjectID, owner: SuiAddress) -> Object {
    Object::with_id_owner_gas_coin_object_for_testing(
        object_id,
        SequenceNumber::new(),
        owner,
        GAS_PER_TX,
    )
}

/// This builds, signs a cert and serializes it
fn make_serialized_cert(
    keys: &[(PublicKeyBytes, KeyPair)],
    committee: &Committee,
    tx: Transaction,
) -> Vec<u8> {
    // Make certificate
    let mut certificate = CertifiedTransaction::new(tx);
    certificate.epoch = committee.epoch();
    for i in 0..committee.quorum_threshold() {
        let (pubx, secx) = keys.get(i).unwrap();
        let sig = AuthoritySignature::new(&certificate.transaction.data, secx);
        certificate.signatures.push((*pubx, sig));
    }

    let serialized_certificate = serialize_cert(&certificate);
    assert!(!serialized_certificate.is_empty());
    serialized_certificate
}

fn make_serialized_transactions(
    address: SuiAddress,
    keypair: KeyPair,
    committee: &Committee,
    account_gas_objects: &[(Vec<Object>, Object)],
    authority_keys: &[(PublicKeyBytes, KeyPair)],
    batch_size: usize,
    use_move: bool,
) -> Vec<Bytes> {
    // Make one transaction per account
    // Depending on benchmark_type, this could be the Order and/or Confirmation.
    account_gas_objects
        .par_iter()
        .map(|(objects, gas_obj)| {
            let next_recipient: SuiAddress = get_key_pair().0;
            let mut single_kinds = vec![];
            for object in objects {
                single_kinds.push(make_transfer_transaction(
                    object.compute_object_reference(),
                    next_recipient,
                    use_move,
                ));
            }
            let gas_object_ref = gas_obj.compute_object_reference();
            let data = if batch_size == 1 {
                TransactionData::new(
                    TransactionKind::Single(single_kinds.into_iter().next().unwrap()),
                    address,
                    gas_object_ref,
                    10000,
                )
            } else {
                assert!(single_kinds.len() == batch_size, "Inconsistent batch size");
                TransactionData::new(
                    TransactionKind::Batch(single_kinds),
                    address,
                    gas_object_ref,
                    2000000,
                )
            };

            let signature = Signature::new(&data, &keypair);
            let transaction = Transaction::new(data, signature);

            // Serialize transaction
            let serialized_transaction = serialize_transaction(&transaction);

            assert!(!serialized_transaction.is_empty());

            vec![
                serialized_transaction.into(),
                make_serialized_cert(authority_keys, committee, transaction).into(),
            ]
        })
        .flatten()
        .collect()
}

pub struct TransactionCreator {
    pub object_id_offset: ObjectID,
}

impl Default for TransactionCreator {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionCreator {
    pub fn new() -> Self {
        Self {
            object_id_offset: ObjectID::from_hex_literal(OBJECT_ID_OFFSET).unwrap(),
        }
    }
    pub fn new_with_offset(object_id_offset: ObjectID) -> Self {
        Self { object_id_offset }
    }

    pub fn generate_transactions(
        &mut self,
        tcp_conns: usize,
        use_move: bool,
        chunk_size: usize,
        num_chunks: usize,
        sender: Option<&KeyPair>,
        validator_preparer: &mut ValidatorPreparer,
    ) -> Vec<Bytes> {
        let (address, keypair) = if let Some(a) = sender {
            (SuiAddress::from(a.public_key_bytes()), a.copy())
        } else {
            get_key_pair()
        };
        let (signed_txns, txn_objects) = self.make_transactions(
            address,
            keypair,
            chunk_size,
            num_chunks,
            tcp_conns,
            use_move,
            self.object_id_offset,
            &validator_preparer.keys,
            &validator_preparer.committee,
        );

        validator_preparer.update_objects_for_validator(txn_objects, address);

        signed_txns
    }

    fn make_gas_objects(
        &mut self,
        address: SuiAddress,
        tx_count: usize,
        batch_size: usize,
        obj_id_offset: ObjectID,
    ) -> Vec<(Vec<Object>, Object)> {
        let total_count = tx_count * batch_size;
        let mut objects = vec![];
        let mut gas_objects = vec![];
        // Objects to be transferred
        ObjectID::in_range(obj_id_offset, total_count as u64)
            .unwrap()
            .iter()
            .for_each(|q| objects.push(create_gas_object(*q, address)));

        // Objects for payment
        let next_offset = objects[objects.len() - 1].id();

        ObjectID::in_range(next_offset.next_increment().unwrap(), tx_count as u64)
            .unwrap()
            .iter()
            .for_each(|q| gas_objects.push(create_gas_object(*q, address)));

        self.object_id_offset = gas_objects[gas_objects.len() - 1]
            .id()
            .next_increment()
            .unwrap();

        objects[..]
            .chunks(batch_size)
            .into_iter()
            .map(|q| q.to_vec())
            .zip(gas_objects.into_iter())
            .collect::<Vec<_>>()
    }

    fn make_transactions(
        &mut self,
        address: SuiAddress,
        key_pair: KeyPair,
        chunk_size: usize,
        num_chunks: usize,
        conn: usize,
        use_move: bool,
        object_id_offset: ObjectID,
        auth_keys: &[(PublicKeyBytes, KeyPair)],
        committee: &Committee,
    ) -> (Vec<Bytes>, Vec<Object>) {
        assert_eq!(chunk_size % conn, 0);
        let batch_size_per_conn = chunk_size / conn;

        // The batch-adjusted number of transactions
        let batch_tx_count = num_chunks * conn;
        // Only need one gas object per batch
        let account_gas_objects: Vec<_> = self.make_gas_objects(
            address,
            batch_tx_count,
            batch_size_per_conn,
            object_id_offset,
        );

        // Bulk load objects
        let all_objects: Vec<_> = account_gas_objects
            .clone()
            .into_iter()
            .flat_map(|(objects, gas)| objects.into_iter().chain(std::iter::once(gas)))
            .collect();

        let serialized_txes = make_serialized_transactions(
            address,
            key_pair,
            committee,
            &account_gas_objects,
            auth_keys,
            batch_size_per_conn,
            use_move,
        );
        (serialized_txes, all_objects)
    }
}
