// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::context_data::db_data_provider::PgManager;

use super::{big_int::BigInt, epoch::Epoch, move_object::MoveObject};
use async_graphql::*;
use sui_types::base_types::ObjectID;

#[derive(Copy, Clone, Enum, PartialEq, Eq)]
pub(crate) enum StakeStatus {
    Active,
    Pending,
    Unstaked,
}

#[derive(Clone, PartialEq, Eq, SimpleObject)]
#[graphql(complex)]
pub(crate) struct Stake {
    /// Stake object address
    pub id: ID,
    /// The epoch at which the stake became actives
    #[graphql(skip)]
    pub active_epoch_id: Option<u64>,
    /// The estimated reward for this stake object
    pub estimated_reward: Option<BigInt>,
    /// The principal value of this stake
    pub principal: Option<BigInt>,

    #[graphql(skip)]
    pub request_epoch_id: Option<u64>,
    /// The status of this stake object: Active, Pending, Unstaked
    pub status: Option<StakeStatus>,
    #[graphql(skip)]
    pub staked_sui_id: ObjectID,
}

#[ComplexObject]
impl Stake {
    /// The epoch at which this stake became active
    async fn active_epoch(&self, ctx: &Context<'_>) -> Result<Option<Epoch>> {
        if let Some(epoch_id) = self.active_epoch_id {
            let epoch = ctx
                .data_unchecked::<PgManager>()
                .fetch_epoch_strict(epoch_id)
                .await
                .extend()?;
            Ok(Some(epoch))
        } else {
            Ok(None)
        }
    }

    /// The epoch at which this object was requested to stake
    async fn request_epoch(&self, ctx: &Context<'_>) -> Result<Option<Epoch>> {
        if let Some(epoch_id) = self.request_epoch_id {
            let epoch = ctx
                .data_unchecked::<PgManager>()
                .fetch_epoch_strict(epoch_id)
                .await
                .extend()?;
            Ok(Some(epoch))
        } else {
            Ok(None)
        }
    }

    /// The Stake object as a Move object
    async fn as_move_object(&self, ctx: &Context<'_>) -> Result<Option<MoveObject>> {
        let obj = ctx
            .data_unchecked::<PgManager>()
            .inner
            .get_object_in_blocking_task(self.staked_sui_id)
            .await?;
        Ok(obj.map(|x| MoveObject { native_object: x }))
    }
}
