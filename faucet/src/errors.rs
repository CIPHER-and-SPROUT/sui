// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum FaucetError {
    #[error("Faucet does not have enough balance")]
    InsuffientBalance,

    #[error("Faucet needs at least {0} coins, but only has {1} coin")]
    InsuffientCoins(usize, usize),

    #[error("Fail to split coin: `{0}`")]
    Wallet(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
