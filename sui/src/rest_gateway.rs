// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::anyhow;
use async_trait::async_trait;
use base64ct::{Base64, Encoding};
use dropshot::HttpErrorResponseBody;
use http::StatusCode;
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::TypeTag;
use reqwest::Response;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use sui_core::gateway_state::gateway_responses::TransactionResponse;
use sui_core::gateway_state::{GatewayAPI, GatewayTxSeqNumber};
use sui_types::base_types::{encode_bytes_hex, ObjectID, ObjectRef, SuiAddress, TransactionDigest};
use sui_types::messages::{CallArg, CertifiedTransaction, Transaction, TransactionData};
use sui_types::object::ObjectRead;

use crate::rest_gateway::requests::{
    CallRequest, MergeCoinRequest, PublishRequest, SignedTransaction, SplitCoinRequest,
    TransferTransactionRequest,
};
use crate::rest_gateway::responses::{NamedObjectRef, ObjectResponse, TransactionBytes};

use self::requests::CallRequestArg;

pub mod requests;
pub mod responses;

pub struct RestGatewayClient {
    pub url: String,
}
#[async_trait]
#[allow(unused_variables)]
impl GatewayAPI for RestGatewayClient {
    async fn execute_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionResponse, anyhow::Error> {
        let url = format!("{}/api/", self.url);
        let data = tx.data.to_base64();
        let sig_and_pub_key = format!("{:?}", tx.tx_signature);
        let split = sig_and_pub_key.split('@').collect::<Vec<_>>();
        let signature = split[0];
        let pub_key = split[1];

        let data = SignedTransaction {
            tx_bytes: data,
            signature: signature.to_string(),
            pub_key: pub_key.to_string(),
        };

        Ok(self.post("execute_transaction", data).await?)
    }

    async fn transfer_coin(
        &self,
        signer: SuiAddress,
        object_id: ObjectID,
        gas_payment: ObjectID,
        gas_budget: u64,
        recipient: SuiAddress,
    ) -> Result<TransactionData, anyhow::Error> {
        let object_id = object_id.to_hex();
        let gas_payment = gas_payment.to_hex();

        let request = TransferTransactionRequest {
            from_address: signer.to_string(),
            object_id,
            to_address: recipient.to_string(),
            gas_object_id: gas_payment,
            gas_budget,
        };

        let tx: TransactionBytes = self.post("new_transfer", request).await?;
        Ok(tx.to_data()?)
    }

    async fn sync_account_state(&self, account_addr: SuiAddress) -> Result<(), anyhow::Error> {
        let url = format!("{}/api/sync_account_state", self.url);
        let client = reqwest::Client::new();
        let address = account_addr.to_string();
        let body = json!({ "address": address });

        Self::handle_response_error(client.post(url).body(body.to_string()).send().await?).await?;
        Ok(())
    }

    async fn move_call(
        &self,
        signer: SuiAddress,
        package_object_ref: ObjectRef,
        module: Identifier,
        function: Identifier,
        type_arguments: Vec<TypeTag>,
        gas_object_ref: ObjectRef,
        arguments: Vec<CallArg>,
        gas_budget: u64,
    ) -> Result<TransactionData, anyhow::Error> {
        let type_arg = type_arguments
            .iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>();

        let arguments = arguments
            .iter()
            .map(|arg| match arg {
                CallArg::Pure(bytes) => CallRequestArg::Pure(Base64::encode_string(bytes)),
                CallArg::ImmOrOwnedObject((id, _, _)) => {
                    CallRequestArg::ImmOrOwnedObject(id.to_hex())
                }
                CallArg::SharedObject(id) => CallRequestArg::SharedObject(id.to_hex()),
            })
            .collect();

        let request = CallRequest {
            signer: encode_bytes_hex(&signer),
            package_object_id: package_object_ref.0.to_hex(),
            module: module.into_string(),
            function: function.into_string(),
            type_arguments: Some(type_arg),
            arguments,
            gas_object_id: gas_object_ref.0.to_hex(),
            gas_budget,
        };
        let tx: TransactionBytes = self.post("move_call", request).await?;
        Ok(tx.to_data()?)
    }

    async fn publish(
        &self,
        signer: SuiAddress,
        package_bytes: Vec<Vec<u8>>,
        gas_object_ref: ObjectRef,
        gas_budget: u64,
    ) -> Result<TransactionData, anyhow::Error> {
        let package_bytes = package_bytes
            .iter()
            .map(|s| Base64::encode_string(s))
            .collect::<Vec<_>>();
        let request = PublishRequest {
            sender: encode_bytes_hex(&signer),
            compiled_modules: package_bytes,
            gas_object_id: gas_object_ref.0.to_hex(),
            gas_budget,
        };
        let tx: TransactionBytes = self.post("publish", request).await?;
        Ok(tx.to_data()?)
    }

    async fn split_coin(
        &self,
        signer: SuiAddress,
        coin_object_id: ObjectID,
        split_amounts: Vec<u64>,
        gas_payment: ObjectID,
        gas_budget: u64,
    ) -> Result<TransactionData, anyhow::Error> {
        let request = SplitCoinRequest {
            signer: encode_bytes_hex(&signer),
            coin_object_id: coin_object_id.to_hex(),
            split_amounts,
            gas_payment: gas_payment.to_hex(),
            gas_budget,
        };
        let tx: TransactionBytes = self.post("split_coin", request).await?;
        Ok(tx.to_data()?)
    }

    async fn merge_coins(
        &self,
        signer: SuiAddress,
        primary_coin: ObjectID,
        coin_to_merge: ObjectID,
        gas_payment: ObjectID,
        gas_budget: u64,
    ) -> Result<TransactionData, anyhow::Error> {
        let request = MergeCoinRequest {
            signer: encode_bytes_hex(&signer),
            primary_coin: primary_coin.to_hex(),
            coin_to_merge: coin_to_merge.to_hex(),
            gas_payment: gas_payment.to_hex(),
            gas_budget,
        };
        let tx: TransactionBytes = self.post("merge_coin", request).await?;
        Ok(tx.to_data()?)
    }

    async fn get_object_info(&self, object_id: ObjectID) -> Result<ObjectRead, anyhow::Error> {
        Ok(self
            .get("object_info", "objectId", &object_id.to_hex())
            .await?)
    }

    async fn get_owned_objects(
        &self,
        account_addr: SuiAddress,
    ) -> Result<Vec<ObjectRef>, anyhow::Error> {
        let url = format!("{}/api/objects?address={}", self.url, account_addr);
        let response = reqwest::get(url).await?;
        let response: ObjectResponse = response.json().await?;
        let objects = response
            .objects
            .into_iter()
            .map(NamedObjectRef::to_object_ref)
            .collect::<Result<Vec<_>, anyhow::Error>>()?;
        Ok(objects)
    }

    fn get_total_transaction_number(&self) -> Result<u64, anyhow::Error> {
        // TODO: Implement this.
        Ok(0)
    }

    fn get_transactions_in_range(
        &self,
        start: GatewayTxSeqNumber,
        end: GatewayTxSeqNumber,
    ) -> Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>, anyhow::Error> {
        // TODO: Implement this.
        Ok(vec![])
    }

    fn get_recent_transactions(
        &self,
        count: u64,
    ) -> Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>, anyhow::Error> {
        // TODO: Implement this.
        Ok(vec![])
    }

    async fn get_transaction(
        &self,
        digest: TransactionDigest,
    ) -> Result<CertifiedTransaction, anyhow::Error> {
        let hex_digest = encode_bytes_hex(&digest);
        let url = format!("{}/api/tx?digest={}", self.url, hex_digest);
        let response = reqwest::blocking::get(url)?;
        Ok(response.json()?)
    }
}

impl RestGatewayClient {
    async fn post<T: DeserializeOwned, V: Serialize>(
        &self,
        endpoint: &str,
        data: V,
    ) -> Result<T, anyhow::Error> {
        let url = format!("{}/api/{endpoint}", self.url);
        let client = reqwest::Client::new();
        let value = serde_json::to_value(data)?;
        let response = client.post(url).body(value.to_string()).send().await?;
        Ok(Self::handle_response_error(response).await?.json().await?)
    }

    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        key: &str,
        value: &str,
    ) -> Result<T, anyhow::Error> {
        let url = format!("{}/api/{endpoint}?{key}={value}", self.url);
        let response = reqwest::get(url).await?;
        Ok(Self::handle_response_error(response).await?.json().await?)
    }

    async fn handle_response_error(response: Response) -> Result<Response, anyhow::Error> {
        if response.status() < StatusCode::BAD_REQUEST {
            Ok(response)
        } else {
            let error: HttpErrorResponseBody = response.json().await?;
            Err(anyhow!("Gateway error response: {}", error.message))
        }
    }
}
