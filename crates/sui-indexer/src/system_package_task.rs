// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use sui_types::SYSTEM_PACKAGE_ADDRESSES;
use tokio_util::sync::CancellationToken;

use crate::{indexer_reader::IndexerReader, schema::epochs};

/// Background task responsible for evicting system packages from the package resolver's cache after
/// detecting an epoch boundary.
pub(crate) struct SystemPackageTask {
    /// Holds the DB connection and also the package resolver to evict packages from.
    reader: IndexerReader,
    /// Signal to cancel the task.
    cancel: CancellationToken,
    /// Interval to sleep for between checks.
    interval: Duration,
}

impl SystemPackageTask {
    pub(crate) fn new(
        reader: IndexerReader,
        cancel: CancellationToken,
        interval: Duration,
    ) -> Self {
        Self {
            reader,
            cancel,
            interval,
        }
    }

    pub(crate) async fn run(&self) {
        let mut last_epoch: i64 = 0;
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    tracing::info!(
                        "Shutdown signal received, terminating system package eviction task"
                    );
                    return;
                }
                _ = tokio::time::sleep(self.interval) => {
                    let next_epoch: i64 = match self.reader.spawn_blocking(move |this| {
                        this.run_query(|conn| {
                            epochs::dsl::epochs
                                .select(epochs::dsl::epoch)
                                .order_by(epochs::epoch.desc())
                                .first(conn)
                        })
                    }).await {
                        Ok(epoch) => epoch,
                        Err(e) => {
                            tracing::error!("Failed to fetch latest epoch: {:?}", e);
                            continue;
                        }
                    };

                    if next_epoch > last_epoch {
                        last_epoch = next_epoch;
                        self.reader
                            .package_resolver()
                            .package_store()
                            .evict(SYSTEM_PACKAGE_ADDRESSES.iter().copied());
                    }
                }
            }
        }
    }
}
