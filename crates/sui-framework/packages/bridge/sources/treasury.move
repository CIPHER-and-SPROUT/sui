// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module bridge::treasury {
    use std::ascii;
    use std::ascii::String;
    use std::type_name;
    use std::type_name::TypeName;

    use sui::address;
    use sui::bag;
    use sui::bag::Bag;
    use sui::coin::{Self, Coin, TreasuryCap, CoinMetadata};
    use sui::event::emit;
    use sui::math;
    use sui::object;
    use sui::object_bag::{Self, ObjectBag};
    use sui::package;
    use sui::package::UpgradeCap;
    use sui::vec_map;
    use sui::vec_map::VecMap;

    const EUnsupportedTokenType: u64 = 0;
    const EInvalidUpgradeCap: u64 = 1;
    const ETokenSupplyNonZero: u64 = 2;

    #[test_only]
    const USD_VALUE_MULTIPLIER: u64 = 10000; // 4 DP accuracy

    public struct BridgeTreasury has store {
        // token treasuries, values are TreasuryCaps for native bridge V1, it can also store Vaults for native tokens in future release.
        treasuries: ObjectBag,
        supported_tokens: VecMap<TypeName, BridgeTokenMetadata>,
        // Mapping token id to type name
        id_token_type_map: VecMap<u8, TypeName>,
        // Bag for storing potential new token waiting to be approved
        waiting_room: Bag
    }

    public struct BridgeTokenMetadata has store, copy, drop {
        id: u8,
        decimal_multiplier: u64,
        notional_value: u64,
        native_token: bool
    }

    public struct ForeignTokenRegistration has store {
        type_name: TypeName,
        uc: UpgradeCap,
        decimal: u8,
    }

    public struct UpdateTokenPriceEvent has copy, drop {
        token_id: u8,
        new_price: u64,
    }

    public struct NewTokenEvent has copy, drop {
        token_id: u8,
        type_name: TypeName,
        native_token: bool
    }

    public struct TokenRegistrationEvent has copy, drop {
        type_name: TypeName,
        decimal:u8,
        native_token: bool
    }

    public(package) fun register_foreign_token<T>(self: &mut BridgeTreasury, tc: TreasuryCap<T>, uc: UpgradeCap, metadata: &CoinMetadata<T>) {
        // Make sure TreasuryCap has not been minted before.
        assert!(coin::total_supply(&tc) == 0, ETokenSupplyNonZero);
        let type_name = type_name::get<T>();
        let coin_address = address::from_ascii_bytes(ascii::as_bytes(&type_name::get_address(&type_name)));
        // Make sure upgrade cap is for the Coin package
        assert!(object::id_to_address(&package::upgrade_package(&uc)) == coin_address, EInvalidUpgradeCap);
        let registration = ForeignTokenRegistration {
            type_name,
            uc,
            decimal: coin::get_decimals(metadata),
        };
        bag::add(&mut self.waiting_room, type_name::into_string(type_name), registration);
        object_bag::add(&mut self.treasuries, type_name, tc);

        emit(TokenRegistrationEvent{
            type_name,
            decimal: coin::get_decimals(metadata),
            native_token: false
        });
    }

    public fun token_id<T>(self: &BridgeTreasury): u8 {
        let metadata = self.get_token_metadata<T>();
        metadata.id
    }

    public fun decimal_multiplier<T>(self: &BridgeTreasury): u64 {
        let metadata = self.get_token_metadata<T>();
        metadata.decimal_multiplier
    }

    public fun notional_value<T>(self: &BridgeTreasury): u64 {
        let metadata = self.get_token_metadata<T>();
        metadata.notional_value
    }

    public(package) fun approve_new_token(self: &mut BridgeTreasury, token_name: String, token_id:u8, native_token: bool, notional_value: u64) {
        if (!native_token){
            let ForeignTokenRegistration{
                type_name,
                uc,
                decimal,
            } = bag::remove<String, ForeignTokenRegistration>(&mut self.waiting_room, token_name);
            vec_map::insert(&mut self.supported_tokens, type_name, BridgeTokenMetadata{
                id: token_id,
                decimal_multiplier: math::pow(10, decimal),
                notional_value,
                native_token
            });
            vec_map::insert(&mut self.id_token_type_map, token_id, type_name);

            // Freeze upgrade cap to prevent changes to the coin
            transfer::public_freeze_object(uc);

            emit(NewTokenEvent{
                token_id,
                type_name,
                native_token
            })
        } else {
            // Not implemented for V1
        }
    }

    public(package) fun create(ctx: &mut TxContext): BridgeTreasury {
        BridgeTreasury {
            treasuries: object_bag::new(ctx),
            supported_tokens: vec_map::empty(),
            id_token_type_map: vec_map::empty(),
            waiting_room: bag::new(ctx),
        }
    }

    public(package) fun burn<T>(self: &mut BridgeTreasury, token: Coin<T>) {
        let treasury = &mut self.treasuries[type_name::get<T>()];
        coin::burn(treasury, token);
    }

    public(package) fun mint<T>(self: &mut BridgeTreasury, amount: u64, ctx: &mut TxContext): Coin<T> {
        let treasury = &mut self.treasuries[type_name::get<T>()];
        coin::mint(treasury, amount, ctx)
    }

    public(package) fun update_asset_notional_price(self: &mut BridgeTreasury, token_id: u8, new_usd_price: u64) {
        let type_name = self.id_token_type_map.try_get(&token_id);
        assert!(type_name.is_some(), EUnsupportedTokenType);
        let type_name = type_name.destroy_some();
        let metadata = self.supported_tokens.get_mut(&type_name);
        metadata.notional_value = new_usd_price;

        emit(UpdateTokenPriceEvent {
            token_id,
            new_price: new_usd_price,
        })
    }

    fun get_token_metadata<T>(self: &BridgeTreasury): BridgeTokenMetadata {
        let coin_type = type_name::get<T>();
        let metadata = self.supported_tokens.try_get(&coin_type);
        assert!(metadata.is_some(), EUnsupportedTokenType);
        metadata.destroy_some()
    }

    #[test_only]
    public struct ETH {}
    #[test_only]
    public struct BTC {}
    #[test_only]
    public struct USDT {}
    #[test_only]
    public struct USDC {}

    #[test_only]
    public fun mock_for_test(ctx: &mut TxContext): BridgeTreasury {
        let mut treasury = create(ctx);

        treasury.supported_tokens.insert(type_name::get<BTC>(), BridgeTokenMetadata{
            id: 1,
            decimal_multiplier: 100_000_000,
            notional_value: 50_000 * USD_VALUE_MULTIPLIER,
            native_token: false,
        });
        treasury.supported_tokens.insert(type_name::get<ETH>(), BridgeTokenMetadata{
            id: 2,
            decimal_multiplier: 100_000_000,
            notional_value: 3_000 * USD_VALUE_MULTIPLIER,
            native_token: false,
        });
        treasury.supported_tokens.insert(type_name::get<USDC>(), BridgeTokenMetadata{
            id: 3,
            decimal_multiplier: 1_000_000,
            notional_value: USD_VALUE_MULTIPLIER,
            native_token: false,
        });
        treasury.supported_tokens.insert(type_name::get<USDT>(), BridgeTokenMetadata{
            id: 4,
            decimal_multiplier: 1_000_000,
            notional_value: USD_VALUE_MULTIPLIER,
            native_token: false,
        });

        treasury.id_token_type_map.insert(1, type_name::get<BTC>());
        treasury.id_token_type_map.insert(2, type_name::get<ETH>());
        treasury.id_token_type_map.insert(3, type_name::get<USDC>());
        treasury.id_token_type_map.insert(4, type_name::get<USDT>());
        treasury
    }
}
