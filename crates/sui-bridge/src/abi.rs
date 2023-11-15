// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use ethers::contract::abigen;

pub enum EthBridgeEvent {
    ExampleContract(ExampleContractEvents),
}

abigen!(
    ExampleContract,
    "abi/example.json",
    event_derives(serde::Deserialize, serde::Serialize)
);
