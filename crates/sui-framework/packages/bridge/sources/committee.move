// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[allow(unused_use)]
module bridge::committee {
    use std::vector;

    use sui::ecdsa_k1;
    use sui::hex;
    use sui::tx_context::{Self, TxContext};
    use sui::vec_map::{Self, VecMap};
    use sui::vec_set;

    use bridge::message::{Self, BridgeMessage};
    use bridge::message_types;

    const ESignatureBelowThreshold: u64 = 0;
    const EDuplicatedSignature: u64 = 1;
    const EInvalidSignature: u64 = 2;
    const ENotSystemAddress: u64 = 3;

    const SUI_MESSAGE_PREFIX: vector<u8> = b"SUI_BRIDGE_MESSAGE";

    struct BridgeCommittee has store {
        // commitee pub key and weight
        pub_keys: VecMap<vector<u8>, u64>,
        // threshold for each message type
        thresholds: VecMap<u8, u64>
    }

    public fun create(ctx: &TxContext): BridgeCommittee {
        assert!(tx_context::sender(ctx) == @0x0, ENotSystemAddress);

        // Hardcoded genesis committee
        let pub_keys = vec_map::empty();
        vec_map::insert(
            &mut pub_keys,
            hex::decode(b"029bef8d556d80e43ae7e0becb3a7e6838b95defe45896ed6075bb9035d06c9964"),
            10
        );
        vec_map::insert(
            &mut pub_keys,
            hex::decode(b"033e99a541db69bd32040dfe5037fbf5210dafa8151a71e21c5204b05d95ce0a62"),
            10
        );

        let thresholds = vec_map::empty();
        vec_map::insert(&mut thresholds, message_types::token(), 10);

        BridgeCommittee {
            pub_keys,
            thresholds
        }
    }

    public fun verify_signatures(
        self: &BridgeCommittee,
        message: BridgeMessage,
        signatures: vector<vector<u8>>,
    ) {
        let (i, signature_counts) = (0, vector::length(&signatures));
        let seen_pub_key = vec_set::empty<vector<u8>>();
        let required_threshold = *vec_map::get(&self.thresholds, &message::message_type(&message));

        // add prefix to the message bytes
        let message_bytes = SUI_MESSAGE_PREFIX;
        vector::append(&mut message_bytes, message::serialise_message(message));

        let threshold = 0;
        while (vec_set::size(&seen_pub_key) < signature_counts) {
            let signature = vector::borrow(&signatures, i);
            let pubkey = ecdsa_k1::secp256k1_ecrecover(signature, &message_bytes, 0);
            // check duplicate
            assert!(!vec_set::contains(&seen_pub_key, &pubkey), EDuplicatedSignature);
            // make sure pub key is part od the committee
            assert!(vec_map::contains(&self.pub_keys, &pubkey), EInvalidSignature);
            // get committee signature weight and check pubkey is part of the committee
            let weight = vec_map::get(&self.pub_keys, &pubkey);
            threshold = threshold + *weight;
            i = i + 1;
            vec_set::insert(&mut seen_pub_key, pubkey);
        };
        assert!(threshold >= required_threshold, ESignatureBelowThreshold);
    }

    #[test_only]
    const TEST_MSG: vector<u8> =
        b"00010a0000000000000000200000000000000000000000000000000000000000000000000000000000000064012000000000000000000000000000000000000000000000000000000000000000c8033930000000000000";

    #[test]
    fun test_verify_signatures_good_path() {
        let committee = setup_test();
        let msg = message::deserialise_message(hex::decode(TEST_MSG));
        // good path
        verify_signatures(
            &committee,
            msg,
            vector[hex::decode(
                b"8ba030a450cb1e36f61e572645fc9da1dea5f79b6db663a21ab63286d7fc29af447433abdd0c0b35ab751154ac5b612ae64d3be810f0d9e10ff68e764514ced300"
            ), hex::decode(
                b"439379cc7b3ee3ebe1ff59d011dafc1caac47da6919b089c90f6a24e8c284b963b20f1f5421385456e57ac6b69c4b5f0d345aa09b8bc96d88d87051c7349e83801"
            )],
        );

        // Clean up
        let BridgeCommittee {
            pub_keys: _,
            thresholds: _
        } = committee;
    }

    #[test]
    #[expected_failure(abort_code = EDuplicatedSignature)]
    fun test_verify_signatures_duplicated_sig() {
        let committee = setup_test();
        let msg = message::deserialise_message(hex::decode(TEST_MSG));
        // good path
        verify_signatures(
            &committee,
            msg,
            vector[hex::decode(
                b"439379cc7b3ee3ebe1ff59d011dafc1caac47da6919b089c90f6a24e8c284b963b20f1f5421385456e57ac6b69c4b5f0d345aa09b8bc96d88d87051c7349e83801"
            ), hex::decode(
                b"439379cc7b3ee3ebe1ff59d011dafc1caac47da6919b089c90f6a24e8c284b963b20f1f5421385456e57ac6b69c4b5f0d345aa09b8bc96d88d87051c7349e83801"
            )],
        );
        abort 0
    }

    #[test]
    #[expected_failure(abort_code = EInvalidSignature)]
    fun test_verify_signatures_invalid_signature() {
        let committee = setup_test();
        let msg = message::deserialise_message(hex::decode(TEST_MSG));
        // good path
        verify_signatures(
            &committee,
            msg,
            vector[hex::decode(
                b"6ffb3e5ce04dd138611c49520fddfbd6778879c2db4696139f53a487043409536c369c6ffaca165ce3886723cfa8b74f3e043e226e206ea25e313ea2215e6caf01"
            )],
        );
        abort 0
    }

    #[test]
    #[expected_failure(abort_code = ESignatureBelowThreshold)]
    fun test_verify_signatures_below_threshold() {
        let committee = setup_test();
        let msg = message::deserialise_message(hex::decode(TEST_MSG));
        // good path
        verify_signatures(
            &committee,
            msg,
            vector[hex::decode(
                b"439379cc7b3ee3ebe1ff59d011dafc1caac47da6919b089c90f6a24e8c284b963b20f1f5421385456e57ac6b69c4b5f0d345aa09b8bc96d88d87051c7349e83801"
            )],
        );
        abort 0
    }

    #[test_only]
    fun setup_test(): BridgeCommittee {
        let pub_keys = vec_map::empty<vector<u8>, u64>();
        vec_map::insert(
            &mut pub_keys,
            hex::decode(b"029bef8d556d80e43ae7e0becb3a7e6838b95defe45896ed6075bb9035d06c9964"),
            100
        );
        vec_map::insert(
            &mut pub_keys,
            hex::decode(b"033e99a541db69bd32040dfe5037fbf5210dafa8151a71e21c5204b05d95ce0a62"),
            100
        );

        let thresholds = vec_map::empty<u8, u64>();
        vec_map::insert(&mut thresholds, message_types::token(), 200);

        let committee = BridgeCommittee {
            pub_keys,
            thresholds
        };
        committee
    }
}
