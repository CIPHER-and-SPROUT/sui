// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use aws_sdk_dynamodb as dynamodb;
use aws_sdk_dynamodb::config::{Credentials, Region};
use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::{AttributeValue, PutRequest, WriteRequest};
use serde::Serialize;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use sui_config::node::KVStoreConfig;

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub enum KVTable {
    Transactions,
    Effects,
    Events,
    State,
}

const UPLOAD_PROGRESS_KEY: [u8; 1] = [0];

#[async_trait]
pub trait KVWriteClient {
    async fn multi_set<V: Serialize>(
        &mut self,
        table: KVTable,
        values: impl IntoIterator<Item = (Vec<u8>, V)> + std::marker::Send,
    ) -> anyhow::Result<()>;
    async fn get_state(&self) -> anyhow::Result<Option<u64>>;
    async fn update_state(&mut self, value: u64) -> anyhow::Result<()>;

    fn deserialize_state(bytes: Vec<u8>) -> u64 {
        let mut array: [u8; 8] = [0; 8];
        array.copy_from_slice(&bytes);
        u64::from_be_bytes(array)
    }
}

pub struct DynamoDbClient {
    client: dynamodb::Client,
    table_name: String,
}

impl DynamoDbClient {
    pub async fn new(config: &KVStoreConfig) -> Self {
        let credentials = Credentials::new(
            &config.aws_access_key_id,
            &config.aws_secret_access_key,
            None,
            None,
            "dynamodb",
        );
        let aws_config = aws_config::from_env()
            .credentials_provider(credentials)
            .region(Region::new(config.aws_region.clone()))
            .load()
            .await;
        let client = dynamodb::Client::new(&aws_config);
        Self {
            client,
            table_name: config.table_name.clone(),
        }
    }

    fn type_name(table: KVTable) -> String {
        match table {
            KVTable::Transactions => "tx",
            KVTable::Effects => "fx",
            KVTable::Events => "ev",
            KVTable::State => "state",
        }
        .to_string()
    }
}

#[async_trait]
impl KVWriteClient for DynamoDbClient {
    async fn multi_set<V: Serialize>(
        &mut self,
        table: KVTable,
        values: impl IntoIterator<Item = (Vec<u8>, V)> + std::marker::Send,
    ) -> anyhow::Result<()> {
        let mut items = vec![];
        let mut seen = HashSet::new();
        for (digest, value) in values {
            if seen.contains(&digest) {
                continue;
            }
            seen.insert(digest.clone());
            let item = WriteRequest::builder()
                .set_put_request(Some(
                    PutRequest::builder()
                        .item("digest", AttributeValue::B(Blob::new(digest)))
                        .item("type", AttributeValue::S(Self::type_name(table)))
                        .item(
                            "bcs",
                            AttributeValue::B(Blob::new(bcs::to_bytes(value.borrow())?)),
                        )
                        .build(),
                ))
                .build();
            items.push(item);
        }
        if items.is_empty() {
            return Ok(());
        }
        for chunk in items.chunks(25) {
            self.client
                .batch_write_item()
                .set_request_items(Some(HashMap::from([(
                    self.table_name.clone(),
                    chunk.to_vec(),
                )])))
                .send()
                .await?;
        }
        Ok(())
    }

    async fn get_state(&self) -> anyhow::Result<Option<u64>> {
        let item = self
            .client
            .get_item()
            .table_name(self.table_name.clone())
            .key("digest", AttributeValue::B(Blob::new(UPLOAD_PROGRESS_KEY)))
            .key("type", AttributeValue::S("state".to_string()))
            .send()
            .await?;
        if let Some(output) = item.item() {
            if let AttributeValue::B(progress) = &output["value"] {
                return Ok(Some(bcs::from_bytes(&progress.clone().into_inner())?));
            }
        }
        Ok(None)
    }

    async fn update_state(&mut self, value: u64) -> anyhow::Result<()> {
        self.client
            .put_item()
            .table_name(self.table_name.clone())
            .item("digest", AttributeValue::B(Blob::new(UPLOAD_PROGRESS_KEY)))
            .item("type", AttributeValue::S("state".to_string()))
            .item(
                "value",
                AttributeValue::B(Blob::new(bcs::to_bytes(&value)?)),
            )
            .send()
            .await?;
        Ok(())
    }
}

// #[derive(Default)]
// pub(crate) struct InMemoryKVClient {
//     data: HashMap<KVTable, HashMap<String, Vec<u8>>>,
// }
//
// #[async_trait]
// impl KVWriteClient for InMemoryKVClient {
//     async fn multi_set<V: Serialize>(
//         &mut self,
//         table: KVTable,
//         values: impl IntoIterator<Item = (String, V)> + std::marker::Send,
//     ) -> anyhow::Result<()> {
//         if let Some(table) = self.data.get_mut(&table) {
//             table.extend(Self::serialize_values(values)?)
//         }
//         Ok(())
//     }
//
//     async fn get_state(&self, state: State) -> anyhow::Result<Option<u64>> {
//         let value = self.data[&KVTable::State]
//             .get(&state.to_string())
//             .map(|bytes| {
//                 let mut array: [u8; 8] = [0; 8];
//                 array.copy_from_slice(bytes);
//                 u64::from_be_bytes(array)
//             });
//         Ok(value)
//     }
//
//     async fn update_state(&mut self, state: State, value: u64) -> anyhow::Result<()> {
//         self.data
//             .entry(KVTable::State)
//             .or_insert_with(HashMap::new)
//             .insert(state.to_string(), u64::to_be_bytes(value).to_vec());
//         Ok(())
//     }
// }
