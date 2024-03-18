// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Coin<SUI> is the token used to pay for gas in Sui.
/// It has 9 decimals, and the smallest unit (10^-9) is called "mist".
module sui::sui {
    use std::option;
    use sui::tx_context::{Self, TxContext};
    use sui::balance::{Self, Balance};
    use sui::transfer;
    use sui::coin;

    const EAlreadyMinted: u64 = 0;
    /// Sender is not @0x0 the system address.
    const ENotSystemAddress: u64 = 1;

    /// The amount of Mist per Sui token based on the the fact that mist is
    /// 10^-9 of a Sui token
    const MIST_PER_SUI: u64 = 1_000_000_000;

    /// The total supply of Sui denominated in whole Sui tokens (10 Billion)
    const TOTAL_SUPPLY_SUI: u64 = 10_000_000_000;

    /// The total supply of Sui denominated in Mist (10 Billion * 10^9)
    const TOTAL_SUPPLY_MIST: u64 = 10_000_000_000_000_000_000;

    /// Name of the coin
    struct SUI has drop {}

    #[allow(unused_function)]
    /// Register the `SUI` Coin to acquire its `Supply`.
    /// This should be called only once during genesis creation.
    fun new(ctx: &mut TxContext): Balance<SUI> {
        assert!(tx_context::sender(ctx) == @0x0, ENotSystemAddress);
        assert!(tx_context::epoch(ctx) == 0, EAlreadyMinted);

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
        transfer::public_freeze_object(metadata);
        let supply = coin::treasury_into_supply(treasury);
        let total_sui = balance::increase_supply(&mut supply, TOTAL_SUPPLY_MIST);
        balance::destroy_supply(supply);
        total_sui
    }

    public entry fun transfer(c: coin::Coin<SUI>, recipient: address) {
        transfer::public_transfer(c, recipient)
    }

    #[test_only]    
    /// Initiates the Sui Coin in tests.
    /// It allows the `sui::coin::CoinMetadata` to be accessible in test modules. 
    /// The Sui Coin decimals is often needed in DeFi to perform fixed-point math operations.  
    /// The functions `sui::coin::mint_for_testing` and `sui::balance::create_for_testing` allow 
    /// the user to mint Sui but does not share its metadata.
    public fun new_for_testing(ctx: &mut TxContext): Balance<SUI> {
        new(ctx)
    }
}
