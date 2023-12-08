// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::context_data::db_data_provider::PgManager;

use super::big_int::BigInt;
use super::move_object::MoveObject;
use super::sui_address::SuiAddress;
use super::validator_credentials::ValidatorCredentials;
use super::{address::Address, base64::Base64};
use async_graphql::*;
use sui_types::base_types::SuiAddress as NativeSuiAddress;

use sui_types::sui_system_state::sui_system_state_summary::SuiValidatorSummary as NativeSuiValidatorSummary;
#[derive(Clone, Debug)]
pub(crate) struct Validator {
    pub validator_summary: NativeSuiValidatorSummary,
    pub at_risk_validators: Option<BTreeMap<NativeSuiAddress, u64>>,
    pub report_records: Option<BTreeMap<NativeSuiAddress, Vec<NativeSuiAddress>>>,
}

#[Object]
impl Validator {
    /// The validator's address.
    async fn address(&self) -> Address {
        Address {
            address: SuiAddress::from(self.validator_summary.sui_address),
        }
    }

    /// Validator's set of credentials.
    async fn credentials(&self) -> Option<ValidatorCredentials> {
        let v = &self.validator_summary;
        let credentials = ValidatorCredentials {
            protocol_pub_key: Some(Base64::from(v.protocol_pubkey_bytes.clone())),
            network_pub_key: Some(Base64::from(v.network_pubkey_bytes.clone())),
            worker_pub_key: Some(Base64::from(v.worker_pubkey_bytes.clone())),
            proof_of_possession: Some(Base64::from(v.proof_of_possession_bytes.clone())),
            net_address: Some(v.net_address.clone()),
            p2p_address: Some(v.p2p_address.clone()),
            primary_address: Some(v.primary_address.clone()),
            worker_address: Some(v.worker_address.clone()),
        };
        Some(credentials)
    }

    /// Validator's set of credentials for the next epoch.
    async fn next_epoch_credentials(&self) -> Option<ValidatorCredentials> {
        let v = &self.validator_summary;
        let credentials = ValidatorCredentials {
            protocol_pub_key: Some(Base64::from(v.protocol_pubkey_bytes.clone())),
            network_pub_key: Some(Base64::from(v.network_pubkey_bytes.clone())),
            worker_pub_key: Some(Base64::from(v.worker_pubkey_bytes.clone())),
            proof_of_possession: Some(Base64::from(v.proof_of_possession_bytes.clone())),
            net_address: Some(v.net_address.clone()),
            p2p_address: Some(v.p2p_address.clone()),
            primary_address: Some(v.primary_address.clone()),
            worker_address: Some(v.worker_address.clone()),
        };
        Some(credentials)
    }

    /// Validator's name.
    async fn name(&self) -> Option<String> {
        Some(self.validator_summary.name.clone())
    }

    /// Validator's description.
    async fn description(&self) -> Option<String> {
        Some(self.validator_summary.description.clone())
    }

    /// Validator's url containing their custom image.
    async fn image_url(&self) -> Option<String> {
        Some(self.validator_summary.image_url.clone())
    }

    /// Validator's homepage URL.
    async fn project_url(&self) -> Option<String> {
        Some(self.validator_summary.project_url.clone())
    }

    /// Number of exchange rates in the table.
    async fn exchange_rates_size(&self) -> Option<u64> {
        Some(self.validator_summary.exchange_rates_size)
    }

    /// The epoch at which this pool became active.
    async fn staking_pool_activation_epoch(&self) -> Option<u64> {
        self.validator_summary.staking_pool_activation_epoch
    }

    /// The total number of SUI tokens in this pool.
    async fn staking_pool_sui_balance(&self) -> Option<BigInt> {
        Some(BigInt::from(
            self.validator_summary.staking_pool_sui_balance,
        ))
    }

    /// The epoch stake rewards will be added here at the end of each epoch.
    async fn rewards_pool(&self) -> Option<BigInt> {
        Some(BigInt::from(self.validator_summary.rewards_pool))
    }

    /// Total number of pool tokens issued by the pool.
    async fn pool_token_balance(&self) -> Option<BigInt> {
        Some(BigInt::from(self.validator_summary.pool_token_balance))
    }

    /// Pending stake amount for this epoch.
    async fn pending_stake(&self) -> Option<BigInt> {
        Some(BigInt::from(self.validator_summary.pending_stake))
    }

    /// Pending stake withdrawn during the current epoch, emptied at epoch boundaries.
    async fn pending_total_sui_withdraw(&self) -> Option<BigInt> {
        Some(BigInt::from(
            self.validator_summary.pending_total_sui_withdraw,
        ))
    }

    async fn pending_pool_token_withdraw(&self) -> Option<BigInt> {
        Some(BigInt::from(
            self.validator_summary.pending_pool_token_withdraw,
        ))
    }

    async fn voting_power(&self) -> Option<u64> {
        Some(self.validator_summary.voting_power)
    }

    // TODO async fn stake_units(&self) -> Option<u64>{}

    async fn gas_price(&self) -> Option<BigInt> {
        Some(BigInt::from(self.validator_summary.gas_price))
    }

    async fn commission_rate(&self) -> Option<u64> {
        Some(self.validator_summary.commission_rate)
    }

    async fn next_epoch_stake(&self) -> Option<BigInt> {
        Some(BigInt::from(self.validator_summary.next_epoch_stake))
    }

    async fn next_epoch_gas_price(&self) -> Option<BigInt> {
        Some(BigInt::from(self.validator_summary.next_epoch_gas_price))
    }

    async fn next_epoch_commission_rate(&self) -> Option<u64> {
        Some(self.validator_summary.next_epoch_commission_rate)
    }

    /// The number of epochs for which this validator has been below the
    /// low stake threshold.
    async fn at_risk(&self) -> Option<u64> {
        match &self.at_risk_validators {
            Some(x) => x
                .get(&self.validator_summary.sui_address)
                .map(|v| v.clone()),
            None => None,
        }
    }

    // only available on sui_system_state_summary
    /// The addresses of other validators this validator has reported
    async fn report_records(&self) -> Option<Vec<Address>> {
        match &self.report_records {
            Some(x) => x.get(&self.validator_summary.sui_address).map(|v| {
                v.into_iter()
                    .map(|a| Address {
                        address: SuiAddress::from_array(a.to_inner()),
                    })
                    .collect()
            }),
            None => None,
        }
    }

    // TODO async fn apy(&self) -> Option<u64>{}

    async fn operation_cap(&self, ctx: &Context<'_>) -> Result<Option<MoveObject>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_move_obj(self.operation_cap_id(), None)
            .await
            .extend()
    }

    async fn staking_pool(&self, ctx: &Context<'_>) -> Result<Option<MoveObject>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_move_obj(self.staking_pool_id(), None)
            .await
            .extend()
    }

    async fn exchange_rates(&self, ctx: &Context<'_>) -> Result<Option<MoveObject>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_move_obj(self.exchange_rates_id(), None)
            .await
            .extend()
    }
}

impl Validator {
    pub fn operation_cap_id(&self) -> SuiAddress {
        SuiAddress::from_array(**self.validator_summary.operation_cap_id)
    }
    pub fn staking_pool_id(&self) -> SuiAddress {
        SuiAddress::from_array(**self.validator_summary.staking_pool_id)
    }
    pub fn exchange_rates_id(&self) -> SuiAddress {
        SuiAddress::from_array(**self.validator_summary.exchange_rates_id)
    }
}
