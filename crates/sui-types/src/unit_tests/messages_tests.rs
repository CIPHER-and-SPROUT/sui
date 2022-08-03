// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use narwhal_crypto::traits::AggregateAuthenticator;
use narwhal_crypto::traits::KeyPair;
use roaring::RoaringBitmap;

use crate::crypto::bcs_signable_test::{get_obligation_input, Foo};
use crate::crypto::{get_key_pair, AccountKeyPair, AuthorityKeyPair, AuthorityPublicKeyBytes};
use crate::object::Owner;

use super::*;
fn random_object_ref() -> ObjectRef {
    (
        ObjectID::random(),
        SequenceNumber::new(),
        ObjectDigest::new([0; 32]),
    )
}

#[test]
fn test_signed_values() {
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    // TODO: refactor this test to not reuse the same keys for user and authority signing
    let (_a1, sec1): (_, AuthorityKeyPair) = get_key_pair();
    let (_a2, sec2): (_, AuthorityKeyPair) = get_key_pair();
    let (_a3, sec3): (_, AuthorityKeyPair) = get_key_pair();
    let (a_sender, sender_sec): (_, AccountKeyPair) = get_key_pair();
    let (_a_sender2, sender_sec2): (_, AccountKeyPair) = get_key_pair();

    authorities.insert(
        /* address */ AuthorityPublicKeyBytes::from(sec1.public()),
        /* voting right */ 1,
    );
    authorities.insert(
        /* address */ AuthorityPublicKeyBytes::from(sec2.public()),
        /* voting right */ 0,
    );
    let committee = Committee::new(0, authorities).unwrap();

    let transaction = Transaction::from_data(
        TransactionData::new_transfer(
            _a2,
            random_object_ref(),
            a_sender,
            random_object_ref(),
            10000,
        ),
        &sender_sec,
    );
    let bad_transaction = Transaction::from_data(
        TransactionData::new_transfer(
            _a2,
            random_object_ref(),
            a_sender,
            random_object_ref(),
            10000,
        ),
        &sender_sec2,
    );

    let v = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        AuthorityPublicKeyBytes::from(sec1.public()),
        &sec1,
    );
    assert!(v.verify(&committee).is_ok());

    let v = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        AuthorityPublicKeyBytes::from(sec2.public()),
        &sec2,
    );
    assert!(v.verify(&committee).is_err());

    let v = SignedTransaction::new(
        committee.epoch(),
        transaction,
        AuthorityPublicKeyBytes::from(sec3.public()),
        &sec3,
    );
    assert!(v.verify(&committee).is_err());

    let v = SignedTransaction::new(
        committee.epoch(),
        bad_transaction,
        AuthorityPublicKeyBytes::from(sec1.public()),
        &sec1,
    );
    assert!(v.verify(&committee).is_err());
}

#[test]
fn test_certificates() {
    let (_a1, sec1): (_, AuthorityKeyPair) = get_key_pair();
    let (a2, sec2): (_, AuthorityKeyPair) = get_key_pair();
    let (_a3, sec3): (_, AuthorityKeyPair) = get_key_pair();
    let (a_sender, sender_sec): (_, AccountKeyPair) = get_key_pair();
    let (_a_sender2, sender_sec2): (_, AccountKeyPair) = get_key_pair();

    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    authorities.insert(
        /* address */ AuthorityPublicKeyBytes::from(sec1.public()),
        /* voting right */ 1,
    );
    authorities.insert(
        /* address */ AuthorityPublicKeyBytes::from(sec2.public()),
        /* voting right */ 1,
    );
    let committee = Committee::new(0, authorities).unwrap();

    let transaction = Transaction::from_data(
        TransactionData::new_transfer(
            a2,
            random_object_ref(),
            a_sender,
            random_object_ref(),
            10000,
        ),
        &sender_sec,
    );
    let bad_transaction = Transaction::from_data(
        TransactionData::new_transfer(
            a2,
            random_object_ref(),
            a_sender,
            random_object_ref(),
            10000,
        ),
        &sender_sec2,
    );

    let v1 = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        AuthorityPublicKeyBytes::from(sec1.public()),
        &sec1,
    );
    let v2 = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        AuthorityPublicKeyBytes::from(sec2.public()),
        &sec2,
    );
    let v3 = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        AuthorityPublicKeyBytes::from(sec3.public()),
        &sec3,
    );

    let mut builder = SignatureAggregator::try_new(transaction.clone(), &committee).unwrap();
    assert!(builder
        .append(
            v1.auth_sign_info.authority,
            v1.auth_sign_info.signature.clone()
        )
        .unwrap()
        .is_none());
    let c = builder
        .append(v2.auth_sign_info.authority, v2.auth_sign_info.signature)
        .unwrap()
        .unwrap();

    assert!(c.verify(&committee).is_ok());

    let mut builder = SignatureAggregator::try_new(transaction, &committee).unwrap();
    assert!(builder
        .append(v1.auth_sign_info.authority, v1.auth_sign_info.signature)
        .unwrap()
        .is_none());
    assert!(builder
        .append(v3.auth_sign_info.authority, v3.auth_sign_info.signature)
        .is_err());

    assert!(SignatureAggregator::try_new(bad_transaction, &committee).is_err());
}

#[test]
fn test_new_with_signatures() {
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();

    for _ in 0..5 {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        signatures.push((AuthorityPublicKeyBytes::from(sec.public()), sig));
        authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);
    }
    let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
    authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(signatures.clone(), &committee).unwrap();

    let sig_clone = signatures.clone();
    let mut alphabetical_authorities = sig_clone
        .iter()
        .map(|(pubx, _)| pubx)
        .collect::<Vec<&AuthorityName>>();
    alphabetical_authorities.sort();
    assert_eq!(
        quorum
            .authorities(&committee)
            .collect::<SuiResult<Vec<&AuthorityName>>>()
            .unwrap(),
        alphabetical_authorities
    );
}

#[test]
fn test_handle_reject_malicious_signature() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();

    for i in 0..5 {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);
        if i < 4 {
            signatures.push((AuthorityPublicKeyBytes::from(sec.public()), sig))
        };
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(signatures, &committee).unwrap();
    {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&message, &sec);
        quorum.signature.add_signature(sig).unwrap();
    }
    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_ok());
    assert!(obligation.verify_all().is_err());
}

#[test]
fn test_bitmap_out_of_range() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);
        signatures.push((AuthorityPublicKeyBytes::from(sec.public()), sig));
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(signatures, &committee).unwrap();

    // Insert outside of range
    quorum.signers_map.insert(10);

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_err());
}

#[test]
fn test_reject_extra_public_key() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);
        signatures.push((AuthorityPublicKeyBytes::from(sec.public()), sig));
    }

    signatures.sort_by_key(|k| k.0);

    let used_signatures: Vec<(AuthorityName, AuthoritySignature)> = vec![
        signatures[0].clone(),
        signatures[1].clone(),
        signatures[2].clone(),
        signatures[3].clone(),
    ];

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(used_signatures, &committee).unwrap();

    quorum.signers_map.insert(3);

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_ok());
}

#[test]
fn test_reject_reuse_signatures() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);
        signatures.push((AuthorityPublicKeyBytes::from(sec.public()), sig));
    }

    let used_signatures: Vec<(AuthorityName, AuthoritySignature)> = vec![
        signatures[0].clone(),
        signatures[1].clone(),
        signatures[2].clone(),
        signatures[2].clone(),
    ];

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(used_signatures, &committee).unwrap();

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_err());
}

#[test]
fn test_empty_bitmap() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec): (_, AuthorityKeyPair) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(AuthorityPublicKeyBytes::from(sec.public()), 1);
        signatures.push((AuthorityPublicKeyBytes::from(sec.public()), sig));
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(signatures, &committee).unwrap();
    quorum.signers_map = RoaringBitmap::new();

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_err());
}

#[test]
fn test_digest_caching() {
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    // TODO: refactor this test to not reuse the same keys for user and authority signing
    let (a1, sec1): (_, AuthorityKeyPair) = get_key_pair();
    let (_a2, sec2): (_, AuthorityKeyPair) = get_key_pair();

    let (sa1, _ssec1): (_, AccountKeyPair) = get_key_pair();
    let (sa2, ssec2): (_, AccountKeyPair) = get_key_pair();

    authorities.insert(sec1.public().into(), 1);
    authorities.insert(sec2.public().into(), 0);

    let committee = Committee::new(0, authorities).unwrap();

    let transaction = Transaction::from_data(
        TransactionData::new_transfer(sa1, random_object_ref(), sa2, random_object_ref(), 10000),
        &ssec2,
    );

    let mut signed_tx = SignedTransaction::new(
        committee.epoch(),
        transaction,
        AuthorityPublicKeyBytes::from(sec1.public()),
        &sec1,
    );
    assert!(signed_tx.verify(&committee).is_ok());

    let initial_digest = *signed_tx.digest();

    signed_tx.data.gas_budget += 1;

    // digest is cached
    assert_eq!(initial_digest, *signed_tx.digest());

    let serialized_tx = bincode::serialize(&signed_tx).unwrap();

    let deserialized_tx: SignedTransaction = bincode::deserialize(&serialized_tx).unwrap();

    // cached digest was not serialized/deserialized
    assert_ne!(initial_digest, *deserialized_tx.digest());

    let effects = TransactionEffects {
        status: ExecutionStatus::Success,
        gas_used: GasCostSummary {
            computation_cost: 0,
            storage_cost: 0,
            storage_rebate: 0,
        },
        shared_objects: Vec::new(),
        transaction_digest: initial_digest,
        created: Vec::new(),
        mutated: Vec::new(),
        unwrapped: Vec::new(),
        deleted: Vec::new(),
        wrapped: Vec::new(),
        gas_object: (random_object_ref(), Owner::AddressOwner(a1)),
        events: Vec::new(),
        dependencies: Vec::new(),
    };

    let mut signed_effects = effects.to_sign_effects(
        committee.epoch(),
        &AuthorityPublicKeyBytes::from(sec1.public()),
        &sec1,
    );

    let initial_effects_digest = *signed_effects.digest();
    signed_effects.effects.gas_used.computation_cost += 1;

    // digest is cached
    assert_eq!(initial_effects_digest, *signed_effects.digest());

    let serialized_effects = bincode::serialize(&signed_effects).unwrap();

    let deserialized_effects: SignedTransactionEffects =
        bincode::deserialize(&serialized_effects).unwrap();

    // cached digest was not serialized/deserialized
    assert_ne!(initial_effects_digest, *deserialized_effects.digest());
}
