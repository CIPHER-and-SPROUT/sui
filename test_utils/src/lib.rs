// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
pub mod authority;
pub mod messages;
pub mod network;
pub mod objects;
pub mod sequencer;

use rand::rngs::StdRng;
use rand::SeedableRng;
use sui_types::base_types::SuiAddress;
use sui_types::committee::Committee;
use sui_types::crypto::{get_key_pair_from_rng, KeyPair};

/// The size of the committee used for tests.
pub const TEST_COMMITTEE_SIZE: usize = 4;

/// Generate `COMMITTEE_SIZE` test cryptographic key pairs.
pub fn test_keys() -> Vec<(SuiAddress, KeyPair)> {
    let mut rng = StdRng::from_seed([0; 32]);
    (0..TEST_COMMITTEE_SIZE)
        .map(|_| get_key_pair_from_rng(&mut rng))
        .collect()
}

/// Generate a test Sui committee with `TEST_COMMITTEE_SIZE` members.
pub fn test_committee() -> Committee {
    Committee::new(
        test_keys()
            .into_iter()
            .map(|(_, x)| (*x.public_key_bytes(), /* voting right */ 1))
            .collect(),
    )
}
