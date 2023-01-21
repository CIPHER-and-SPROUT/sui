// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Coin<SUI> is the token used to pay for gas in Sui.
/// It has 9 decimals, and the smallest unit (10^-9) is called "mist".
module sui::sui {
    use std::option;
    use sui::tx_context::TxContext;
    use sui::balance::Supply;
    use sui::transfer;
    use sui::coin;
    use sui::event;

    friend sui::genesis;

    /// Name of the coin
    struct SUI has drop {}

    struct Canary has copy, drop { value: u64 }
    public entry fun coal_mine() {
        event::emit(Canary { value: 42 })
    }

    /// Register the `SUI` Coin to acquire its `Supply`.
    /// This should be called only once during genesis creation.
    public(friend) fun new(ctx: &mut TxContext): Supply<SUI> {
        let (treasury, metadata) = coin::create_currency(
            SUI {}, 
            9,
            b"SUI",
            b"Sui",
            // TODO: add appropriate description and logo url
            b"",
            option::none(),
            ctx
        );
        transfer::freeze_object(metadata);
        coin::treasury_into_supply(treasury)
    }

    public entry fun transfer(c: coin::Coin<SUI>, recipient: address) {
        transfer::transfer(c, recipient)
    }
}
