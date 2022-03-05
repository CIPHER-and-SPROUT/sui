// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::authority::AuthorityState;
use std::io;
use sui_network::{
    network::NetworkServer,
    transport::{spawn_server, MessageHandler, SpawnedServer, RwChannel},
};
use sui_types::{error::*, messages::*, serialize::*};

use futures::{StreamExt, SinkExt};

use tracing::*;

use async_trait::async_trait;

pub struct AuthorityServer {
    server: NetworkServer,
    pub state: AuthorityState,
}

impl AuthorityServer {
    pub fn new(
        base_address: String,
        base_port: u16,
        buffer_size: usize,
        state: AuthorityState,
    ) -> Self {
        Self {
            server: NetworkServer::new(base_address, base_port, buffer_size),
            state,
        }
    }

    pub async fn spawn(self) -> Result<SpawnedServer, io::Error> {
        let address = format!("{}:{}", self.server.base_address, self.server.base_port);
        let buffer_size = self.server.buffer_size;

        // Launch server for the appropriate protocol.
        spawn_server(&address, self, buffer_size).await
    }

    fn handle_one_message<'a>(
        &'a self,
        buffer: &'a [u8],
    ) -> futures::future::BoxFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let result = deserialize_message(buffer);
            let reply = match result {
                Err(_) => Err(SuiError::InvalidDecoding),
                Ok(result) => {
                    match result {
                        SerializedMessage::Transaction(message) => self
                            .state
                            .handle_transaction(*message)
                            .await
                            .map(|info| Some(serialize_transaction_info(&info))),
                        SerializedMessage::Cert(message) => {
                            let confirmation_transaction = ConfirmationTransaction {
                                certificate: message.as_ref().clone(),
                            };
                            match self
                                .state
                                .handle_confirmation_transaction(confirmation_transaction)
                                .await
                            {
                                Ok(info) => {
                                    // Response
                                    Ok(Some(serialize_transaction_info(&info)))
                                }
                                Err(error) => Err(error),
                            }
                        }
                        SerializedMessage::AccountInfoReq(message) => self
                            .state
                            .handle_account_info_request(*message)
                            .await
                            .map(|info| Some(serialize_account_info_response(&info))),
                        SerializedMessage::ObjectInfoReq(message) => self
                            .state
                            .handle_object_info_request(*message)
                            .await
                            .map(|info| Some(serialize_object_info_response(&info))),
                        SerializedMessage::TransactionInfoReq(message) => self
                            .state
                            .handle_transaction_info_request(*message)
                            .await
                            .map(|info| Some(serialize_transaction_info(&info))),
                        _ => Err(SuiError::UnexpectedMessage),
                    }
                }
            };

            self.server.increment_packets_processed();

            if self.server.packets_processed() % 5000 == 0 {
                info!(
                    "{}:{} has processed {} packets",
                    self.server.base_address,
                    self.server.base_port,
                    self.server.packets_processed()
                );
            }

            match reply {
                Ok(x) => x,
                Err(error) => {
                    warn!("User query failed: {}", error);
                    self.server.increment_user_errors();
                    Some(serialize_error(&error))
                }
            }
        })
    }
}

#[async_trait]
impl<'a, A> MessageHandler<A> for AuthorityServer where 
    A : 'static + RwChannel<'a> + Unpin + Send {

        async fn handle_message(
            &self,
            mut channel : A,
        ) -> () {

                loop {
                    let buffer = match channel.stream().next().await {
                        Some(Ok(buffer)) => buffer,
                        Some(Err(err)) => {
                            // We expect some EOF or disconnect error at the end.
                            error!("Error while reading TCP stream: {}", err);
                            break;
                        },
                        None => {
                            break;
                        }
                    };
    
                    if let Some(reply) = self.handle_one_message(&buffer[..], ).await {
                        let status = channel.sink().send(reply.into()).await;
                        if let Err(error) = status {
                            error!("Failed to send query response: {}", error);
                        }
                    };
                }

        }
}
