// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use std::sync::Arc;

use bytes::Bytes;
use consensus_config::{AuthorityIndex, NetworkKeyPair};
use serde::{Deserialize, Serialize};

use crate::{block::BlockRef, error::ConsensusResult};

mod anemo_network;

/// Network client for communicating with peers.
#[async_trait]
pub(crate) trait NetworkClient: Send + Sync {
    /// Sends a serialized SignedBlock to a peer.
    async fn send_block(&self, peer: AuthorityIndex, block: &Bytes) -> ConsensusResult<()>;

    /// Fetches serialized `SignedBlock`s from a peer.
    async fn fetch_blocks(
        &self,
        peer: AuthorityIndex,
        block_refs: Vec<BlockRef>,
    ) -> ConsensusResult<Vec<Bytes>>;
}

/// Network service for handling requests from peers.
#[async_trait]
pub(crate) trait NetworkService: Send + Sync {
    async fn handle_send_block(&self, peer: AuthorityIndex, block: Bytes) -> ConsensusResult<()>;
    async fn handle_fetch_blocks(
        &self,
        peer: AuthorityIndex,
        block_refs: Vec<BlockRef>,
    ) -> ConsensusResult<Vec<Bytes>>;
}

/// An `AuthorityNode` holds a `NetworkManager` until shutdown.
/// Dropping `NetworkManager` will shutdown the network service.
pub(crate) trait NetworkManager<C, S>
where
    C: NetworkClient,
    S: NetworkService,
{
    /// Returns the network client.
    fn client(&self) -> Arc<C>;

    /// Installs network service.
    fn install_service(&self, network_signer: NetworkKeyPair, service: Box<S>);
}

/// Network message types.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SendBlockRequest {
    // Serialized SignedBlock.
    block: Bytes,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SendBlockResponse {}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FetchBlocksRequest {
    block_refs: Vec<BlockRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FetchBlocksResponse {
    // Serialized SignedBlock.
    blocks: Vec<Bytes>,
}
