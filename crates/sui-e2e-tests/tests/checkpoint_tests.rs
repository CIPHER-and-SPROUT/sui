// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use sui_macros::register_fail_point;
use sui_macros::register_fail_point_if;
use sui_macros::sim_test;
use sui_test_transaction_builder::make_transfer_sui_transaction;
use test_cluster::TestClusterBuilder;
use tracing::debug;

#[sim_test]
async fn basic_checkpoints_integration_test() {
    let test_cluster = TestClusterBuilder::new().build().await;
    let tx = make_transfer_sui_transaction(&test_cluster.wallet, None, None).await;
    let digest = *tx.digest();
    test_cluster.execute_transaction(tx).await;

    for _ in 0..600 {
        let all_included = test_cluster
            .swarm
            .validator_node_handles()
            .into_iter()
            .all(|handle| {
                handle.with(|node| {
                    node.state()
                        .epoch_store_for_testing()
                        .is_transaction_executed_in_checkpoint(&digest)
                        .unwrap()
                })
            });
        if all_included {
            // success
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    panic!("Did not include transaction in checkpoint in 60 seconds");
}

#[sim_test]
async fn checkpoint_split_brain_test() {
    // count number of nodes that have reached split brain condition
    let count_split_brain_nodes: Arc<Mutex<AtomicUsize>> = Default::default();
    let count_clone = count_split_brain_nodes.clone();
    register_fail_point("split_brain_reached", move || {
        let counter = count_clone.lock().unwrap();
        counter.fetch_add(1, Ordering::Relaxed);
    });

    // Create shared list containing node id's that will register split
    // brain condition (create execution nondeterminism).
    // The first two nodes to acquire the lock will add themselves to the list.
    // We do this rather than, e.g., register all even simnode ID's,
    // because in some cases the simnode ID's assigned are not sequential.
    let fail_node_list: Arc<Mutex<Vec<u64>>> = Default::default();
    let fail_list_clone = fail_node_list.clone();
    register_fail_point_if("cp_execution_nondeterminism", move || {
        let mut fail_list = fail_list_clone.lock().unwrap();
        if fail_list.len() < 2 {
            fail_list.push(sui_simulator::current_simnode_id().0);
            true
        } else {
            fail_list.contains(&sui_simulator::current_simnode_id().0)
        }
    });

    let test_cluster = TestClusterBuilder::new()
        .with_num_validators(4)
        .build()
        .await;

    let tx = make_transfer_sui_transaction(&test_cluster.wallet, None, None).await;
    test_cluster.execute_transaction(tx).await;

    // provide enough time for validators to detect split brain
    tokio::time::sleep(Duration::from_secs(5)).await;

    // all honest validators should eventually detect a split brain
    let final_count = count_split_brain_nodes.lock().unwrap();
    assert!(final_count.load(Ordering::Relaxed) >= 2);
}
