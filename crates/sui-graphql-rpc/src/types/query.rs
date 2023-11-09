// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::{
    address::Address,
    checkpoint::{Checkpoint, CheckpointId},
    coin::{Coin, CoinMetadata},
    epoch::Epoch,
    event::{Event, EventFilter},
    object::{Object, ObjectFilter},
    owner::{ObjectOwner, Owner},
    protocol_config::ProtocolConfigs,
    sui_address::SuiAddress,
    sui_system_state_summary::SuiSystemStateSummary,
    transaction_block::{TransactionBlock, TransactionBlockFilter},
};
use crate::{config::ServiceConfig, context_data::db_data_provider::PgManager, error::Error};

pub(crate) struct Query;
pub(crate) type SuiGraphQLSchema = async_graphql::Schema<Query, EmptyMutation, EmptySubscription>;

#[Object]
impl Query {
    /// First four bytes of the network's genesis checkpoint digest (uniquely identifies the
    /// network).
    async fn chain_identifier(&self, ctx: &Context<'_>) -> Result<String> {
        ctx.data_unchecked::<PgManager>()
            .fetch_chain_identifier()
            .await
            .extend()
    }

    /// Configuration for this RPC service
    async fn service_config(&self, ctx: &Context<'_>) -> Result<ServiceConfig> {
        ctx.data()
            .map_err(|_| Error::Internal("Unable to fetch service configuration.".to_string()))
            .cloned()
            .extend()
    }

    // availableRange - pending impl. on IndexerV2
    // dryRunTransactionBlock
    // coinMetadata

    async fn owner(&self, address: SuiAddress) -> Option<ObjectOwner> {
        Some(ObjectOwner::Owner(Owner { address }))
    }

    async fn object(
        &self,
        ctx: &Context<'_>,
        address: SuiAddress,
        version: Option<u64>,
    ) -> Result<Option<Object>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_obj(address, version)
            .await
            .extend()
    }

    async fn address(&self, address: SuiAddress) -> Option<Address> {
        Some(Address { address })
    }

    async fn epoch(&self, ctx: &Context<'_>, id: Option<u64>) -> Result<Option<Epoch>> {
        if let Some(epoch_id) = id {
            ctx.data_unchecked::<PgManager>()
                .fetch_epoch(epoch_id)
                .await
                .extend()
        } else {
            Ok(Some(
                ctx.data_unchecked::<PgManager>()
                    .fetch_latest_epoch()
                    .await
                    .extend()?,
            ))
        }
    }

    async fn checkpoint(
        &self,
        ctx: &Context<'_>,
        id: Option<CheckpointId>,
    ) -> Result<Option<Checkpoint>> {
        if let Some(id) = id {
            match (&id.digest, &id.sequence_number) {
                (Some(_), Some(_)) => Err(Error::InvalidCheckpointQuery.extend()),
                _ => ctx
                    .data_unchecked::<PgManager>()
                    .fetch_checkpoint(id.digest.as_deref(), id.sequence_number)
                    .await
                    .extend(),
            }
        } else {
            Ok(Some(
                ctx.data_unchecked::<PgManager>()
                    .fetch_latest_checkpoint()
                    .await
                    .extend()?,
            ))
        }
    }

    async fn transaction_block(
        &self,
        ctx: &Context<'_>,
        digest: String,
    ) -> Result<Option<TransactionBlock>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_tx(&digest)
            .await
            .extend()
    }

    /// The coin objects that exist in the network.
    /// The type field is a string of the inner type of the coin
    /// by which to filter. If no type is provided, it will use the default SUI coin type,
    /// 0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI.
    async fn coin_connection(
        &self,
        ctx: &Context<'_>,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
        type_: Option<String>,
    ) -> Result<Option<Connection<String, Coin>>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_coins(None, type_, first, after, last, before)
            .await
            .extend()
    }

    async fn checkpoint_connection(
        &self,
        ctx: &Context<'_>,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
    ) -> Result<Option<Connection<String, Checkpoint>>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_checkpoints(first, after, last, before, None)
            .await
            .extend()
    }

    async fn transaction_block_connection(
        &self,
        ctx: &Context<'_>,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
        filter: Option<TransactionBlockFilter>,
    ) -> Result<Option<Connection<String, TransactionBlock>>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_txs(first, after, last, before, filter)
            .await
            .extend()
    }

    async fn event_connection(
        &self,
        ctx: &Context<'_>,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
        filter: EventFilter,
    ) -> Result<Option<Connection<String, Event>>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_events(first, after, last, before, filter)
            .await
            .extend()
    }

    async fn object_connection(
        &self,
        ctx: &Context<'_>,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
        filter: Option<ObjectFilter>,
    ) -> Result<Option<Connection<String, Object>>> {
        ctx.data_unchecked::<PgManager>()
            .fetch_objs(first, after, last, before, filter)
            .await
            .extend()
    }

    async fn protocol_config(
        &self,
        ctx: &Context<'_>,
        protocol_version: Option<u64>,
    ) -> Result<ProtocolConfigs> {
        ctx.data_unchecked::<PgManager>()
            .fetch_protocol_configs(protocol_version)
            .await
            .extend()
    }

    /// Resolves the owner address of the provided domain name
    async fn resolve_name_service_address(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> Result<Option<Address>> {
        ctx.data_unchecked::<PgManager>()
            .resolve_name_service_address(ctx.data_unchecked::<NameServiceConfig>(), name)
            .await
            .extend()
    }

    async fn latest_sui_system_state(&self, ctx: &Context<'_>) -> Result<SuiSystemStateSummary> {
        ctx.data_unchecked::<PgManager>()
            .fetch_latest_sui_system_state()
            .await
            .extend()
    }

    async fn coin_metadata(
        &self,
        ctx: &Context<'_>,
        coin_type: String,
    ) -> Result<Option<CoinMetadata>> {
        let coin_struct = parse_to_struct_tag(&coin_type)?;
        let Some(coin_metadata) = ctx
            .data_unchecked::<PgManager>()
            .inner
            .get_coin_metadata_in_blocking_task(coin_struct.clone())
            .await
            .map_err(|e| Error::Internal(e.to_string()))
            .extend()?
        else {
            return Ok(None);
        };

        // pass in the object to CoinMetadata, and lift these fields to the top-level?

        Ok(Some(CoinMetadata {
            decimals: Some(coin_metadata.decimals),
            name: Some(coin_metadata.name.clone()),
            symbol: Some(coin_metadata.symbol.clone()),
            description: Some(coin_metadata.description.clone()),
            icon_url: coin_metadata.icon_url.clone(),
            coin_type,
        }))
    }
}
