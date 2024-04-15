// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use mysten_metrics::{monitored_scope, spawn_monitored_task};
use rand::{
    rngs::{OsRng, StdRng},
    Rng, SeedableRng,
};
use sui_macros::fail_point_async;
use tokio::{
    sync::{mpsc::UnboundedReceiver, oneshot, Semaphore},
    time::sleep,
};
use tracing::{error, error_span, info, trace, Instrument};

use crate::authority::AuthorityState;
use crate::transaction_manager::PendingCertificate;

#[cfg(test)]
#[path = "unit_tests/execution_driver_tests.rs"]
mod execution_driver_tests;

// Execution should not encounter permanent failures, so any failure can and needs
// to be retried.
pub const EXECUTION_MAX_ATTEMPTS: u32 = 10;
const EXECUTION_FAILURE_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const QUEUEING_DELAY_SAMPLING_RATIO: f64 = 0.05;

/// When a notification that a new pending transaction is received we activate
/// processing the transaction in a loop.
pub async fn execution_process(
    authority_state: Weak<AuthorityState>,
    mut rx_ready_certificates: UnboundedReceiver<PendingCertificate>,
    mut rx_execution_shutdown: oneshot::Receiver<()>,
) {
    info!("Starting pending certificates execution process.");

    // Rate limit concurrent executions to # of cpus.
    let limit = Arc::new(Semaphore::new(num_cpus::get()));
    let mut rng = StdRng::from_rng(&mut OsRng).unwrap();

    // Loop whenever there is a signal that a new transactions is ready to process.
    loop {
        let _scope = monitored_scope("ExecutionDriver::loop");

        let certificate;
        let expected_effects_digest;
        let txn_ready_time;
        let epoch;
        tokio::select! {
            result = rx_ready_certificates.recv() => {
                if let Some(pending_cert) = result {
                    certificate = pending_cert.certificate;
                    expected_effects_digest = pending_cert.expected_effects_digest;
                    txn_ready_time = pending_cert.stats.ready_time.unwrap();
                    epoch = pending_cert.epoch;
                } else {
                    // Should only happen after the AuthorityState has shut down and tx_ready_certificate
                    // has been dropped by TransactionManager.
                    info!("No more certificate will be received. Exiting executor ...");
                    return;
                };
            }
            _ = &mut rx_execution_shutdown => {
                info!("Shutdown signal received. Exiting executor ...");
                return;
            }
        };

        let authority = if let Some(authority) = authority_state.upgrade() {
            authority
        } else {
            // Terminate the execution if authority has already shutdown, even if there can be more
            // items in rx_ready_certificates.
            info!("Authority state has shutdown. Exiting ...");
            return;
        };
        authority.metrics.execution_driver_dispatch_queue.dec();

        let digest = *certificate.digest();
        trace!(?digest, "Pending certificate execution activated.");

        // A single execution driver process runs across epochs. After epoch change,
        // transactions from the previous epoch cannot be executed correctly so it is better
        // to skip execution gracefully.
        // Mismatched epoch should not happen on validators or on fullnode with checkpoint executions.
        // But it is possibe in tests on fullnodes with local executions. The execution failure because
        // of mismatched epoch would not be graceful downstream, so it is better to skip execution here.
        let epoch_store = authority.load_epoch_store_one_call_per_task();
        if epoch != epoch_store.epoch() {
            // This certificate is for a different epoch. Ignore it.
            error!(
                ?digest,
                "Ignoring certificate from different epoch ({epoch} instead of {}) for execution ...",
                epoch_store.epoch()
            );
            continue;
        }

        let limit = limit.clone();
        // hold semaphore permit until task completes. unwrap ok because we never close
        // the semaphore in this context.
        let permit = limit.acquire_owned().await.unwrap();

        if rng.gen_range(0.0..1.0) < QUEUEING_DELAY_SAMPLING_RATIO {
            authority
                .metrics
                .execution_queueing_latency
                .report(txn_ready_time.elapsed());
            if let Some(latency) = authority.metrics.execution_queueing_latency.latency() {
                authority
                    .metrics
                    .execution_queueing_delay_s
                    .observe(latency.as_secs_f64());
            }
        }

        authority.metrics.execution_rate_tracker.lock().record();

        // Certificate execution can take significant time, so run it in a separate task.
        spawn_monitored_task!(async move {
            let _scope = monitored_scope("ExecutionDriver::task");
            let _guard = permit;
            if let Ok(true) = authority.is_tx_already_executed(&digest) {
                return;
            }
            let mut attempts = 0;
            loop {
                fail_point_async!("transaction_execution_delay");
                attempts += 1;
                let res = authority
                    .try_execute_immediately(&certificate, expected_effects_digest, &epoch_store)
                    .await;
                if let Err(e) = res {
                    if attempts == EXECUTION_MAX_ATTEMPTS {
                        panic!("Failed to execute certified transaction {digest:?} after {attempts} attempts! error={e} certificate={certificate:?}");
                    }
                    // Assume only transient failure can happen. Permanent failure is probably
                    // a bug. There is nothing that can be done to recover from permanent failures.
                    error!(tx_digest=?digest, "Failed to execute certified transaction {digest:?}! attempt {attempts}, {e}");
                    sleep(EXECUTION_FAILURE_RETRY_INTERVAL).await;
                } else {
                    break;
                }
            }
            authority
                .metrics
                .execution_driver_executed_transactions
                .inc();
        }.instrument(error_span!("execution_driver", tx_digest = ?digest)));
    }
}
