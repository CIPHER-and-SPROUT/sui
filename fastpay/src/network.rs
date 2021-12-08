// Copyright (c) Facebook, Inc. and its affiliates.
// SPDX-License-Identifier: Apache-2.0

use crate::transport::*;
use fastpay_core::{authority::*, client::*};
use fastx_types::{base_types::*, error::*, messages::*, serialize::*};

use bytes::Bytes;
use futures::future::FutureExt;
use log::*;
use std::io;
use tokio::time;

pub struct Server {
    network_protocol: NetworkProtocol,
    base_address: String,
    base_port: u32,
    state: AuthorityState,
    buffer_size: usize,
    // Stats
    packets_processed: u64,
    user_errors: u64,
}

impl Server {
    pub fn new(
        network_protocol: NetworkProtocol,
        base_address: String,
        base_port: u32,
        state: AuthorityState,
        buffer_size: usize,
    ) -> Self {
        Self {
            network_protocol,
            base_address,
            base_port,
            state,
            buffer_size,
            packets_processed: 0,
            user_errors: 0,
        }
    }

    pub fn packets_processed(&self) -> u64 {
        self.packets_processed
    }

    pub fn user_errors(&self) -> u64 {
        self.user_errors
    }

    pub async fn spawn(self) -> Result<SpawnedServer, io::Error> {
        info!(
            "Listening to {} traffic on {}:{}",
            self.network_protocol,
            self.base_address,
            self.base_port + self.state.shard_id
        );
        let address = format!(
            "{}:{}",
            self.base_address,
            self.base_port + self.state.shard_id
        );

        let buffer_size = self.buffer_size;
        let protocol = self.network_protocol;
        let state = RunningServerState { server: self };
        // Launch server for the appropriate protocol.
        protocol.spawn_server(&address, state, buffer_size).await
    }
}

struct RunningServerState {
    server: Server,
}

impl MessageHandler for RunningServerState {
    fn handle_message<'a>(
        &'a mut self,
        buffer: &'a [u8],
    ) -> futures::future::BoxFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let result = deserialize_message(buffer);
            let reply = match result {
                Err(_) => Err(FastPayError::InvalidDecoding),
                Ok(result) => {
                    match result {
                        SerializedMessage::Order(message) => self
                            .server
                            .state
                            .handle_order(*message)
                            .map(|info| Some(serialize_info_response(&info))),
                        SerializedMessage::Cert(message) => {
                            let confirmation_order = ConfirmationOrder {
                                certificate: message.as_ref().clone(),
                            };
                            match self
                                .server
                                .state
                                .handle_confirmation_order(confirmation_order)
                            {
                                Ok(info) => {
                                    // Response
                                    Ok(Some(serialize_info_response(&info)))
                                }
                                Err(error) => Err(error),
                            }
                        }
                        SerializedMessage::InfoReq(message) => self
                            .server
                            .state
                            .handle_account_info_request(*message)
                            .map(|info| Some(serialize_info_response(&info))),
                        _ => Err(FastPayError::UnexpectedMessage),
                    }
                }
            };

            self.server.packets_processed += 1;
            if self.server.packets_processed % 5000 == 0 {
                info!(
                    "{}:{} (shard {}) has processed {} packets",
                    self.server.base_address,
                    self.server.base_port + self.server.state.shard_id,
                    self.server.state.shard_id,
                    self.server.packets_processed
                );
            }

            match reply {
                Ok(x) => x,
                Err(error) => {
                    warn!("User query failed: {}", error);
                    self.server.user_errors += 1;
                    Some(serialize_error(&error))
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct Client {
    network_protocol: NetworkProtocol,
    base_address: String,
    base_port: u32,
    num_shards: u32,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
}

impl Client {
    pub fn new(
        network_protocol: NetworkProtocol,
        base_address: String,
        base_port: u32,
        num_shards: u32,
        buffer_size: usize,
        send_timeout: std::time::Duration,
        recv_timeout: std::time::Duration,
    ) -> Self {
        Self {
            network_protocol,
            base_address,
            base_port,
            num_shards,
            buffer_size,
            send_timeout,
            recv_timeout,
        }
    }

    async fn send_recv_bytes_internal(
        &mut self,
        shard: ShardId,
        buf: Vec<u8>,
    ) -> Result<Vec<u8>, io::Error> {
        let address = format!("{}:{}", self.base_address, self.base_port + shard);
        let mut stream = self
            .network_protocol
            .connect(address, self.buffer_size)
            .await?;
        // Send message
        time::timeout(self.send_timeout, stream.write_data(&buf)).await??;
        // Wait for reply
        time::timeout(self.recv_timeout, stream.read_data()).await?
    }

    pub async fn send_recv_bytes(
        &mut self,
        shard: ShardId,
        buf: Vec<u8>,
    ) -> Result<AccountInfoResponse, FastPayError> {
        match self.send_recv_bytes_internal(shard, buf).await {
            Err(error) => Err(FastPayError::ClientIoError {
                error: format!("{}", error),
            }),
            Ok(response) => {
                // Parse reply
                match deserialize_message(&response[..]) {
                    Ok(SerializedMessage::InfoResp(resp)) => Ok(*resp),
                    Ok(SerializedMessage::Error(error)) => Err(*error),
                    Err(_) => Err(FastPayError::InvalidDecoding),
                    _ => Err(FastPayError::UnexpectedMessage),
                }
            }
        }
    }
}

impl AuthorityClient for Client {
    /// Initiate a new transfer to a FastPay or Primary account.
    fn handle_order(&mut self, order: Order) -> AsyncResult<'_, AccountInfoResponse, FastPayError> {
        Box::pin(async move {
            let shard = AuthorityState::get_shard(self.num_shards, order.object_id());
            self.send_recv_bytes(shard, serialize_order(&order)).await
        })
    }

    /// Confirm a transfer to a FastPay or Primary account.
    fn handle_confirmation_order(
        &mut self,
        order: ConfirmationOrder,
    ) -> AsyncResult<'_, AccountInfoResponse, FastPayError> {
        Box::pin(async move {
            let shard =
                AuthorityState::get_shard(self.num_shards, order.certificate.order.object_id());
            self.send_recv_bytes(shard, serialize_cert(&order.certificate))
                .await
        })
    }

    /// Handle information requests for this account.
    fn handle_account_info_request(
        &mut self,
        request: AccountInfoRequest,
    ) -> AsyncResult<'_, AccountInfoResponse, FastPayError> {
        Box::pin(async move {
            let shard = AuthorityState::get_shard(self.num_shards, &request.object_id);
            self.send_recv_bytes(shard, serialize_info_request(&request))
                .await
        })
    }
}

#[derive(Clone)]
pub struct MassClient {
    network_protocol: NetworkProtocol,
    base_address: String,
    base_port: u32,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
    max_in_flight: u64,
}

impl MassClient {
    pub fn new(
        network_protocol: NetworkProtocol,
        base_address: String,
        base_port: u32,
        buffer_size: usize,
        send_timeout: std::time::Duration,
        recv_timeout: std::time::Duration,
        max_in_flight: u64,
    ) -> Self {
        Self {
            network_protocol,
            base_address,
            base_port,
            buffer_size,
            send_timeout,
            recv_timeout,
            max_in_flight,
        }
    }

    async fn run_shard(&self, shard: u32, requests: Vec<Bytes>) -> Result<Vec<Bytes>, io::Error> {
        let address = format!("{}:{}", self.base_address, self.base_port + shard);
        let mut stream = self
            .network_protocol
            .connect(address, self.buffer_size)
            .await?;
        let mut requests = requests.iter();
        let mut in_flight: u64 = 0;
        let mut responses = Vec::new();

        loop {
            while in_flight < self.max_in_flight {
                let request = match requests.next() {
                    None => {
                        if in_flight == 0 {
                            return Ok(responses);
                        }
                        // No more entries to send.
                        break;
                    }
                    Some(request) => request,
                };
                let status = time::timeout(self.send_timeout, stream.write_data(request)).await;
                if let Err(error) = status {
                    error!("Failed to send request: {}", error);
                    continue;
                }
                in_flight += 1;
            }
            if requests.len() % 5000 == 0 && requests.len() > 0 {
                info!("In flight {} Remaining {}", in_flight, requests.len());
            }
            match time::timeout(self.recv_timeout, stream.read_data()).await {
                Ok(Ok(buffer)) => {
                    in_flight -= 1;
                    responses.push(Bytes::from(buffer));
                }
                Ok(Err(error)) => {
                    if error.kind() == io::ErrorKind::UnexpectedEof {
                        info!("Socket closed by server");
                        return Ok(responses);
                    }
                    error!("Received error response: {}", error);
                }
                Err(error) => {
                    error!(
                        "Timeout while receiving response: {} (in flight: {})",
                        error, in_flight
                    );
                }
            }
        }
    }

    /// Spin off one task for each shard based on this authority client.
    pub fn run<I>(&self, sharded_requests: I) -> impl futures::stream::Stream<Item = Vec<Bytes>>
    where
        I: IntoIterator<Item = (ShardId, Vec<Bytes>)>,
    {
        let handles = futures::stream::FuturesUnordered::new();
        for (shard, requests) in sharded_requests {
            let client = self.clone();
            handles.push(
                tokio::spawn(async move {
                    info!(
                        "Sending {} requests to {}:{} (shard {})",
                        client.network_protocol,
                        client.base_address,
                        client.base_port + shard,
                        shard
                    );
                    let responses = client
                        .run_shard(shard, requests)
                        .await
                        .unwrap_or_else(|_| Vec::new());
                    info!(
                        "Done sending {} requests to {}:{} (shard {})",
                        client.network_protocol,
                        client.base_address,
                        client.base_port + shard,
                        shard
                    );
                    responses
                })
                .then(|x| async { x.unwrap_or_else(|_| Vec::new()) }),
            );
        }
        handles
    }
}
