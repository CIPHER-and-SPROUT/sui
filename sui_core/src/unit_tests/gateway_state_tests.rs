// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, path::Path};

use signature::Signer;

use sui_framework::build_move_package_to_bytes;
use sui_types::{crypto::get_key_pair, object::Owner};

use sui_types::gas_coin::GasCoin;
use sui_types::messages::Transaction;
use sui_types::object::{Object, GAS_VALUE_FOR_TESTING};

use super::*;
use crate::authority_aggregator::authority_aggregator_tests::{
    authority_genesis_objects, crate_object_move_transaction, init_local_authorities,
};
use crate::authority_client::LocalAuthorityClient;
use crate::gateway_state::{GatewayAPI, GatewayState};

async fn create_gateway_state(
    genesis_objects: Vec<Vec<Object>>,
) -> GatewayState<LocalAuthorityClient> {
    let all_owners: HashSet<_> = genesis_objects
        .iter()
        .flat_map(|v| v.iter().map(|o| o.get_single_owner().unwrap()))
        .collect();
    let authorities = init_local_authorities(genesis_objects).await;
    let path = tempfile::tempdir().unwrap().into_path();
    let gateway = GatewayState::new_with_authorities(path, authorities).unwrap();
    for owner in all_owners {
        gateway.sync_account_state(owner).await.unwrap();
    }
    gateway
}

#[tokio::test]
async fn test_transfer_coin() {
    let (addr1, key1) = get_key_pair();
    let (addr2, _key2) = get_key_pair();

    let coin_object = Object::with_owner_for_testing(addr1);
    let gas_object = Object::with_owner_for_testing(addr1);

    let genesis_objects =
        authority_genesis_objects(4, vec![coin_object.clone(), gas_object.clone()]);
    let gateway = create_gateway_state(genesis_objects).await;

    let data = gateway
        .transfer_coin(
            addr1,
            coin_object.id(),
            gas_object.id(),
            GAS_VALUE_FOR_TESTING,
            addr2,
        )
        .await
        .unwrap();

    let signature = key1.sign(&data.to_bytes());
    let (_cert, effects) = gateway
        .execute_transaction(Transaction::new(data, signature))
        .await
        .unwrap()
        .to_effect_response()
        .unwrap();
    assert_eq!(effects.mutated.len(), 2);
    assert_eq!(
        effects.mutated_excluding_gas().next().unwrap().1,
        Owner::AddressOwner(addr2)
    );
    assert_eq!(gateway.get_total_transaction_number().unwrap(), 1);
}

#[tokio::test]
async fn test_move_call() {
    let (addr1, key1) = get_key_pair();
    let gas_object = Object::with_owner_for_testing(addr1);
    let genesis_objects = authority_genesis_objects(4, vec![gas_object.clone()]);
    let gateway = create_gateway_state(genesis_objects).await;

    let framework_obj_ref = gateway.get_framework_object_ref().await.unwrap();
    let tx = crate_object_move_transaction(
        addr1,
        &key1,
        addr1,
        100,
        framework_obj_ref,
        gas_object.compute_object_reference(),
    );

    let (_cert, effects) = gateway
        .execute_transaction(tx)
        .await
        .unwrap()
        .to_effect_response()
        .unwrap();
    assert!(effects.status.is_ok());
    assert_eq!(effects.mutated.len(), 1);
    assert_eq!(effects.created.len(), 1);
    assert_eq!(effects.created[0].1, Owner::AddressOwner(addr1));
}

#[tokio::test]
async fn test_publish() {
    let (addr1, key1) = get_key_pair();
    let gas_object = Object::with_owner_for_testing(addr1);
    let genesis_objects = authority_genesis_objects(4, vec![gas_object.clone()]);
    let gateway = create_gateway_state(genesis_objects).await;

    // Provide path to well formed package sources
    let mut path = env!("CARGO_MANIFEST_DIR").to_owned();
    path.push_str("/src/unit_tests/data/object_owner/");

    let compiled_modules = build_move_package_to_bytes(Path::new(&path), false).unwrap();
    let data = gateway
        .publish(
            addr1,
            compiled_modules,
            gas_object.compute_object_reference(),
            GAS_VALUE_FOR_TESTING,
        )
        .await
        .unwrap();

    let signature = key1.sign(&data.to_bytes());
    gateway
        .execute_transaction(Transaction::new(data, signature))
        .await
        .unwrap()
        .to_publish_response()
        .unwrap();
}

#[tokio::test]
async fn test_coin_split() {
    let (addr1, key1) = get_key_pair();

    let coin_object = Object::with_owner_for_testing(addr1);
    let gas_object = Object::with_owner_for_testing(addr1);

    let genesis_objects =
        authority_genesis_objects(4, vec![coin_object.clone(), gas_object.clone()]);
    let gateway = create_gateway_state(genesis_objects).await;

    let split_amounts = vec![100, 200, 300, 400, 500];
    let total_amount: u64 = split_amounts.iter().sum();

    let data = gateway
        .split_coin(
            addr1,
            coin_object.id(),
            split_amounts.clone(),
            gas_object.id(),
            GAS_VALUE_FOR_TESTING,
        )
        .await
        .unwrap();

    let signature = key1.sign(&data.to_bytes());
    let response = gateway
        .execute_transaction(Transaction::new(data, signature))
        .await
        .unwrap()
        .to_split_coin_response()
        .unwrap();

    assert_eq!(
        (coin_object.id(), coin_object.version().increment()),
        (response.updated_coin.id(), response.updated_coin.version())
    );
    assert_eq!(
        (gas_object.id(), gas_object.version().increment()),
        (response.updated_gas.id(), response.updated_gas.version())
    );
    let update_coin = GasCoin::try_from(response.updated_coin.data.try_as_move().unwrap()).unwrap();
    assert_eq!(update_coin.value(), GAS_VALUE_FOR_TESTING - total_amount);
    let split_coin_values = response
        .new_coins
        .iter()
        .map(|o| {
            GasCoin::try_from(o.data.try_as_move().unwrap())
                .unwrap()
                .value()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        split_amounts,
        split_coin_values.into_iter().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_coin_merge() {
    let (addr1, key1) = get_key_pair();

    let coin_object1 = Object::with_owner_for_testing(addr1);
    let coin_object2 = Object::with_owner_for_testing(addr1);
    let gas_object = Object::with_owner_for_testing(addr1);
    let genesis_objects = authority_genesis_objects(
        4,
        vec![
            coin_object1.clone(),
            coin_object2.clone(),
            gas_object.clone(),
        ],
    );
    let gateway = create_gateway_state(genesis_objects).await;

    let data = gateway
        .merge_coins(
            addr1,
            coin_object1.id(),
            coin_object2.id(),
            gas_object.id(),
            GAS_VALUE_FOR_TESTING,
        )
        .await
        .unwrap();

    let signature = key1.sign(&data.to_bytes());
    let response = gateway
        .execute_transaction(Transaction::new(data, signature))
        .await
        .unwrap()
        .to_merge_coin_response()
        .unwrap();

    assert_eq!(
        (coin_object1.id(), coin_object1.version().increment()),
        (response.updated_coin.id(), response.updated_coin.version())
    );
    assert_eq!(
        (gas_object.id(), gas_object.version().increment()),
        (response.updated_gas.id(), response.updated_gas.version())
    );
    let update_coin = GasCoin::try_from(response.updated_coin.data.try_as_move().unwrap()).unwrap();
    assert_eq!(update_coin.value(), GAS_VALUE_FOR_TESTING * 2);
}

#[tokio::test]
async fn test_recent_transactions() -> Result<(), anyhow::Error> {
    let (addr1, key1) = get_key_pair();
    let (addr2, _) = get_key_pair();

    let object1 = Object::with_owner_for_testing(addr1);
    let object2 = Object::with_owner_for_testing(addr1);
    let object3 = Object::with_owner_for_testing(addr1);
    let gas_object = Object::with_owner_for_testing(addr1);
    let genesis_objects = authority_genesis_objects(
        4,
        vec![
            object1.clone(),
            object2.clone(),
            object3.clone(),
            gas_object.clone(),
        ],
    );
    let gateway = create_gateway_state(genesis_objects).await;

    assert_eq!(gateway.get_total_transaction_number()?, 0);
    let mut cnt = 0;
    let mut digests = vec![];
    for obj_id in [object1.id(), object2.id(), object3.id()] {
        let data = gateway
            .transfer_coin(addr1, obj_id, gas_object.id(), 50000, addr2)
            .await
            .unwrap();
        let signature = key1.sign(&data.to_bytes());
        let response = gateway
            .execute_transaction(Transaction::new(data, signature))
            .await?;
        digests.push((cnt, *response.to_effect_response()?.0.digest()));
        cnt += 1;
        assert_eq!(gateway.get_total_transaction_number()?, cnt);
    }
    // start must <= end.
    assert!(gateway.get_transactions_in_range(2, 1).is_err());
    assert!(gateway.get_transactions_in_range(1, 1).unwrap().is_empty());
    // Extends max range allowed.
    assert!(gateway.get_transactions_in_range(1, 100000).is_err());
    let txs = gateway.get_recent_transactions(10)?;
    assert_eq!(txs.len(), 3);
    assert_eq!(txs, digests);
    let txs = gateway.get_transactions_in_range(0, 10)?;
    assert_eq!(txs.len(), 3);
    assert_eq!(txs, digests);

    Ok(())
}
