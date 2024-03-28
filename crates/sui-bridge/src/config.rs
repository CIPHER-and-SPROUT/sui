// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::BridgeAuthorityKeyPair;
use crate::error::BridgeError;
use crate::eth_client::EthClient;
use crate::sui_client::SuiClient;
use crate::types::BridgeAction;
use anyhow::anyhow;
use ethers::types::Address as EthAddress;
use fastcrypto::traits::EncodeDecodeBase64;
use futures::{future, StreamExt};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use sui_config::Config;
use sui_json_rpc_types::Coin;
use sui_sdk::apis::CoinReadApi;
use sui_sdk::{SuiClient as SuiSdkClient, SuiClientBuilder};
use sui_types::base_types::ObjectRef;
use sui_types::base_types::{ObjectID, SuiAddress};
use sui_types::crypto::SuiKeyPair;
use sui_types::event::EventID;
use sui_types::object::Owner;
use tracing::info;

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BridgeNodeConfig {
    /// The port that the server listens on.
    pub server_listen_port: u16,
    /// The port that for metrics server.
    pub metrics_port: u16,
    /// Path of the file where bridge authority key (Secp256k1) is stored as Base64 encoded `privkey`.
    pub bridge_authority_key_path_base64_raw: PathBuf,
    /// Rpc url for Sui fullnode, used for query stuff and submit transactions.
    pub sui_rpc_url: String,
    /// Rpc url for Eth fullnode, used for query stuff.
    pub eth_rpc_url: String,
    /// The eth contract addresses (hex). It must not be empty. It serves two purpose:
    /// 1. validator only signs bridge actions that are generated from these contracts.
    /// 2. for EthSyncer to watch for when `run_client` is true.
    pub eth_addresses: Vec<String>,
    /// Path of the file where bridge client key (any SuiKeyPair) is stored as Base64 encoded `flag || privkey`.
    /// If `run_client` is true, and this is None, then use `bridge_authority_key_path_base64_raw` as client key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_client_key_path_base64_sui_key: Option<PathBuf>,
    /// Whether to run client. If true, `bridge_client_key_path_base64_sui_key`,
    /// and `db_path` needs to be provided.
    pub run_client: bool,
    /// The gas object to use for paying for gas fees for the client. It needs to
    /// be owned by the address associated with bridge client key. If not set
    /// and `run_client` is true, it will query and use the gas object with highest
    /// amount for the account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_client_gas_object: Option<ObjectID>,
    /// Path of the client storage. Required when `run_client` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_path: Option<PathBuf>,
    // TODO: we need to hardcode the starting blocks for eth networks for cold start.
    /// Override the start block number for each eth address. Key must be in `eth_addresses`.
    /// When set, EthSyncer will start from this block number (inclusively) instead of the one in storage.
    /// Key: eth address, Value:  block number to start from
    /// Note: This field should be rarely used. Only use it when you understand how to follow up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eth_bridge_contracts_start_block_override: Option<BTreeMap<String, u64>>,
    /// Override the last processed EventID for bridge module `bridge`.
    /// When set, SuiSyncer will start from this cursor (exclusively) instead of the one in storage.
    /// If the cursor is not found in storage or override, the query will start from genesis.
    /// Key: sui module, Value: last processed EventID (tx_digest, event_seq).
    /// Note 1: This field should be rarely used. Only use it when you understand how to follow up.
    /// Note 2: the EventID needs to be valid, namely it must exist and matches the filter.
    /// Otherwise, it will miss one event because of fullnode Event query semantics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sui_bridge_module_last_processed_event_id_override: Option<EventID>,
    /// A list of approved governance actions. Action in this list will be signed when requested by client.
    pub approved_governance_actions: Vec<BridgeAction>,
}

impl Config for BridgeNodeConfig {}

impl BridgeNodeConfig {
    pub async fn validate(
        &self,
    ) -> anyhow::Result<(BridgeServerConfig, Option<BridgeClientConfig>)> {
        let bridge_authority_key =
            read_bridge_authority_key(&self.bridge_authority_key_path_base64_raw)?;

        // TODO: verify it's part of bridge committee
        let sui_client = Arc::new(SuiClient::<SuiSdkClient>::new(&self.sui_rpc_url).await?);

        // TODO(audit-blocking): verify Sui Chain ID matches bridge Chain ID

        if self.eth_addresses.is_empty() {
            return Err(anyhow!("`eth_addresses` must contain at least one address"));
        }
        let eth_bridge_contracts = self
            .eth_addresses
            .iter()
            .map(|addr| EthAddress::from_str(addr))
            .collect::<Result<Vec<_>, _>>()?;
        let eth_client = Arc::new(
            EthClient::<ethers::providers::Http>::new(
                &self.eth_rpc_url,
                HashSet::from_iter(eth_bridge_contracts.iter().cloned()),
            )
            .await?,
        );
        // TODO(audit-blocking): verify Ethereum Chain ID matches bridge Chain ID

        // Validate approved actions that must be governace actions
        for action in &self.approved_governance_actions {
            if !action.is_governace_action() {
                return Err(anyhow::anyhow!(format!(
                    "{:?}",
                    BridgeError::ActionIsNotGovernanceAction(action.clone())
                )));
            }
        }
        let approved_governance_actions = self.approved_governance_actions.clone();

        let bridge_server_config = BridgeServerConfig {
            key: bridge_authority_key,
            metrics_port: self.metrics_port,
            server_listen_port: self.server_listen_port,
            sui_client: sui_client.clone(),
            eth_client: eth_client.clone(),
            approved_governance_actions,
        };

        if !self.run_client {
            return Ok((bridge_server_config, None));
        }
        // If client is enabled, prepare client config
        let bridge_client_key = if self.bridge_client_key_path_base64_sui_key.is_none() {
            let bridge_client_key =
                read_bridge_authority_key(&self.bridge_authority_key_path_base64_raw)?;
            Ok(SuiKeyPair::from(bridge_client_key))
        } else {
            read_bridge_client_key(self.bridge_client_key_path_base64_sui_key.as_ref().unwrap())
        }?;

        let client_sui_address = SuiAddress::from(&bridge_client_key.public());
        info!("Bridge client sui address: {:?}", client_sui_address);

        // TODO: decide a minimal amount here and return if no coin has enough balance
        let gas_object_id = match self.bridge_client_gas_object {
            Some(id) => id,
            None => {
                let sui_client = SuiClientBuilder::default().build(&self.sui_rpc_url).await?;
                let coin =
                    pick_highest_balance_coin(sui_client.coin_read_api(), client_sui_address, 0)
                        .await?;
                coin.coin_object_id
            }
        };
        let db_path = self
            .db_path
            .clone()
            .ok_or(anyhow!("`db_path` is required when `run_client` is true"))?;

        let mut eth_bridge_contracts_start_block_override = BTreeMap::new();
        match &self.eth_bridge_contracts_start_block_override {
            Some(overrides) => {
                for (addr, block_number) in overrides {
                    let address = EthAddress::from_str(addr)?;
                    if eth_bridge_contracts.contains(&address) {
                        eth_bridge_contracts_start_block_override.insert(address, *block_number);
                    } else {
                        return Err(anyhow!(
                            "Override start block number for address {:?} is not in `eth_addresses`",
                            addr
                        ));
                    }
                }
            }
            None => {}
        }

        let (gas_coin, gas_object_ref, owner) = sui_client
            .get_gas_data_panic_if_not_gas(gas_object_id)
            .await;
        if owner != Owner::AddressOwner(client_sui_address) {
            return Err(anyhow!("Gas object {:?} is not owned by bridge client key's associated sui address {:?}, but {:?}", gas_object_id, client_sui_address, owner));
        }
        info!(
            "Starting bridge client with gas object {:?}, balance: {}",
            gas_object_ref.0,
            gas_coin.value()
        );
        let bridge_client_config = BridgeClientConfig {
            sui_address: client_sui_address,
            key: bridge_client_key,
            gas_object_ref,
            metrics_port: self.metrics_port,
            sui_client: sui_client.clone(),
            eth_client: eth_client.clone(),
            db_path,
            eth_bridge_contracts,
            eth_bridge_contracts_start_block_override,
            sui_bridge_module_last_processed_event_id_override: self
                .sui_bridge_module_last_processed_event_id_override,
        };

        Ok((bridge_server_config, Some(bridge_client_config)))
    }
}

pub struct BridgeServerConfig {
    pub key: BridgeAuthorityKeyPair,
    pub server_listen_port: u16,
    pub metrics_port: u16,
    pub sui_client: Arc<SuiClient<SuiSdkClient>>,
    pub eth_client: Arc<EthClient<ethers::providers::Http>>,
    /// A list of approved governance actions. Action in this list will be signed when requested by client.
    pub approved_governance_actions: Vec<BridgeAction>,
}

// TODO: add gas balance alert threshold
pub struct BridgeClientConfig {
    pub sui_address: SuiAddress,
    pub key: SuiKeyPair,
    pub gas_object_ref: ObjectRef,
    pub metrics_port: u16,
    pub sui_client: Arc<SuiClient<SuiSdkClient>>,
    pub eth_client: Arc<EthClient<ethers::providers::Http>>,
    pub db_path: PathBuf,
    pub eth_bridge_contracts: Vec<EthAddress>,
    pub eth_bridge_contracts_start_block_override: BTreeMap<EthAddress, u64>,
    pub sui_bridge_module_last_processed_event_id_override: Option<EventID>,
}

/// Read Bridge Authority key (Secp256k1KeyPair) from a file.
/// BridgeAuthority key is stored as base64 encoded `privkey`.
pub fn read_bridge_authority_key(path: &PathBuf) -> Result<BridgeAuthorityKeyPair, anyhow::Error> {
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Bridge authority key file not found at path: {:?}",
            path
        ));
    }
    let contents = std::fs::read_to_string(path)?;

    BridgeAuthorityKeyPair::decode_base64(contents.as_str().trim())
        .map_err(|e| anyhow!("Error decoding authority key: {:?}", e))
}

/// Read Bridge client key (any SuiKeyPair) from a file.
/// Read from file as Base64 encoded `flag || privkey`.
pub fn read_bridge_client_key(path: &PathBuf) -> Result<SuiKeyPair, anyhow::Error> {
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Bridge client key file not found at path: {:?}",
            path
        ));
    }
    let contents = std::fs::read_to_string(path)?;

    SuiKeyPair::decode_base64(contents.as_str().trim())
        .map_err(|e| anyhow!("Error decoding authority key: {:?}", e))
}

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BridgeCommitteeConfig {
    pub bridge_authority_port_and_key_path: Vec<(u64, PathBuf)>,
}

impl Config for BridgeCommitteeConfig {}

pub async fn pick_highest_balance_coin(
    coin_read_api: &CoinReadApi,
    address: SuiAddress,
    minimal_amount: u64,
) -> anyhow::Result<Coin> {
    let mut highest_balance = 0;
    let mut highest_balance_coin = None;
    coin_read_api
        .get_coins_stream(address, None)
        .for_each(|coin: Coin| {
            if coin.balance > highest_balance {
                highest_balance = coin.balance;
                highest_balance_coin = Some(coin.clone());
            }
            future::ready(())
        })
        .await;
    if highest_balance_coin.is_none() {
        return Err(anyhow!("No Sui coins found for address {:?}", address));
    }
    if highest_balance < minimal_amount {
        return Err(anyhow!(
            "Found no single coin that has >= {} balance Sui for address {:?}",
            minimal_amount,
            address,
        ));
    }
    Ok(highest_balance_coin.unwrap())
}
