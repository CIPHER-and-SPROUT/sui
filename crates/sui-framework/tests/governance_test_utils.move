// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module sui::governance_test_utils {
    use sui::address;
    use sui::balance;
    use sui::object;
    use sui::sui::SUI;
    use sui::coin::{Self, Coin};
    use sui::staking_pool::{Self, StakedSui, StakingPool};
    use sui::test_utils::assert_eq;
    use sui::tx_context::{Self, TxContext};
    use sui::validator::{Self, Validator};
    use sui::sui_system::{Self, SuiSystemState};
    use sui::test_scenario::{Self, Scenario};
    use sui::validator_set;
    use std::option;
    use std::vector;

    public fun create_validator_for_testing(
        addr: address, init_stake_amount: u64, ctx: &mut TxContext
    ): Validator {
        let validator = validator::new_for_testing(
            addr,
            x"FF",
            x"FF",
            x"FF",
            x"FF",
            b"ValidatorName",
            b"description",
            b"image_url",
            b"project_url",
            b"/ip4/127.0.0.1/tcp/80",
            b"/ip4/127.0.0.1/udp/80",
            b"/ip4/127.0.0.1/udp/80",
            b"/ip4/127.0.0.1/udp/80",
            option::some(balance::create_for_testing<SUI>(init_stake_amount)),
            1,
            0,
            true,
            ctx
        );
        validator
    }

    /// Create a validator set with the given stake amounts
    public fun create_validators_with_stakes(stakes: vector<u64>, ctx: &mut TxContext): vector<Validator> {
        let i = 0;
        let validators = vector[];
        while (i < vector::length(&stakes)) {
            let validator = create_validator_for_testing(address::from_u256((i as u256)), *vector::borrow(&stakes, i), ctx);
            vector::push_back(&mut validators, validator);
            i = i + 1
        };
        validators
    }

    public fun create_sui_system_state_for_testing(
        validators: vector<Validator>, sui_supply_amount: u64, storage_fund_amount: u64, ctx: &mut TxContext
    ) {
        sui_system::create(
            object::new(ctx), // it doesn't matter what ID sui system state has in tests
            validators,
            balance::create_for_testing<SUI>(sui_supply_amount), // sui_supply
            balance::create_for_testing<SUI>(storage_fund_amount), // storage_fund
            1,   // protocol version
            1,   // system state version
            100, // governance_start_epoch, we set this to a big-ish number so that
                 // low stake departure won't start kicking in for testing
            0,   // epoch_start_timestamp_ms
            42,  // epoch_duration_ms, doesn't matter what number we put here
            0,   // stake subsidy
            10,  // stake_subsidy_period_length
            0,   // stake_subsidy_decrease_rate
            ctx,
        )
    }

    public fun set_up_sui_system_state(addrs: vector<address>, scenario: &mut Scenario) {
        let ctx = test_scenario::ctx(scenario);
        let validators = vector::empty();

        while (!vector::is_empty(&addrs)) {
            vector::push_back(
                &mut validators,
                create_validator_for_testing(vector::pop_back(&mut addrs), 100, ctx)
            );
        };

        create_sui_system_state_for_testing(validators, 1000, 0, ctx);
    }

    public fun advance_epoch(scenario: &mut Scenario) {
        advance_epoch_with_reward_amounts(0, 0, scenario);
    }

    public fun advance_epoch_safe_mode(scenario: &mut Scenario) {
        test_scenario::next_tx(scenario, @0x0);
        let new_epoch = tx_context::epoch(test_scenario::ctx(scenario)) + 1;
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);

        let ctx = test_scenario::ctx(scenario);
        sui_system::advance_epoch_safe_mode(&mut system_state, new_epoch, 1, ctx);
        test_scenario::return_shared(system_state);
        test_scenario::next_epoch(scenario, @0x0);
    }

    public fun advance_epoch_with_reward_amounts(
        storage_charge: u64, computation_charge: u64, scenario: &mut Scenario
    ) {
        test_scenario::next_tx(scenario, @0x0);
        let new_epoch = tx_context::epoch(test_scenario::ctx(scenario)) + 1;
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);

        let ctx = test_scenario::ctx(scenario);

        sui_system::advance_epoch_for_testing(&mut system_state, new_epoch, 1, storage_charge, computation_charge, 0, 0, 0, 0, 1, ctx);
        test_scenario::return_shared(system_state);
        test_scenario::next_epoch(scenario, @0x0);
    }

    public fun advance_epoch_with_reward_amounts_and_slashing_rates(
        storage_charge: u64,
        computation_charge: u64,
        reward_slashing_rate: u64,
        scenario: &mut Scenario
    ) {
        test_scenario::next_tx(scenario, @0x0);
        let new_epoch = tx_context::epoch(test_scenario::ctx(scenario)) + 1;
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);

        let ctx = test_scenario::ctx(scenario);

        sui_system::advance_epoch_for_testing(
            &mut system_state, new_epoch, 1, storage_charge, computation_charge, 0, 0, reward_slashing_rate, 0, 1, ctx
        );
        test_scenario::return_shared(system_state);
        test_scenario::next_epoch(scenario, @0x0);
    }

    public fun stake_with(
        staker: address, validator: address, amount: u64, scenario: &mut Scenario
    ) {
        test_scenario::next_tx(scenario, staker);
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);

        let ctx = test_scenario::ctx(scenario);

        sui_system::request_add_stake(&mut system_state, coin::mint_for_testing(amount, ctx), validator, ctx);
        test_scenario::return_shared(system_state);
    }

    public fun unstake(
        staker: address, staked_sui_idx: u64, scenario: &mut Scenario
    ) {
        test_scenario::next_tx(scenario, staker);
        let stake_sui_ids = test_scenario::ids_for_sender<StakedSui>(scenario);
        let staked_sui = test_scenario::take_from_sender_by_id(scenario, *vector::borrow(&stake_sui_ids, staked_sui_idx));
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);

        let ctx = test_scenario::ctx(scenario);
        sui_system::request_withdraw_stake(&mut system_state, staked_sui, ctx);
        test_scenario::return_shared(system_state);
    }

    public fun add_validator_full_flow(validator: address, init_stake_amount: u64, pubkey: vector<u8>, pop: vector<u8>, scenario: &mut Scenario) {
        test_scenario::next_tx(scenario, validator);
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
        let addr = b"/ip4/127.0.0.1/udp/80";
        let ctx = test_scenario::ctx(scenario);

        sui_system::request_add_validator_candidate(
            &mut system_state,
            pubkey,
            vector[171, 2, 39, 3, 139, 105, 166, 171, 153, 151, 102, 197, 151, 186, 140, 116, 114, 90, 213, 225, 20, 167, 60, 69, 203, 12, 180, 198, 9, 217, 117, 38],
            vector[171, 2, 39, 3, 139, 105, 166, 171, 153, 151, 102, 197, 151, 186, 140, 116, 114, 90, 213, 225, 20, 167, 60, 69, 203, 12, 180, 198, 9, 217, 117, 38],
            pop,
            b"name",
            b"description",
            b"image_url",
            b"project_url",
            addr,
            addr,
            addr,
            addr,
            1,
            0,
            ctx
        );
        sui_system::request_add_stake(&mut system_state, coin::mint_for_testing<SUI>(init_stake_amount, ctx), validator, ctx);
        sui_system::request_add_validator_for_testing(&mut system_state, 0, ctx);
        test_scenario::return_shared(system_state);
    }

    public fun add_validator_candidate(validator: address, pubkey: vector<u8>, pop: vector<u8>, scenario: &mut Scenario) {
        test_scenario::next_tx(scenario, validator);
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
        let addr = b"/ip4/127.0.0.1/udp/80";
        let ctx = test_scenario::ctx(scenario);

        sui_system::request_add_validator_candidate(
            &mut system_state,
            pubkey,
            vector[171, 2, 39, 3, 139, 105, 166, 171, 153, 151, 102, 197, 151, 186, 140, 116, 114, 90, 213, 225, 20, 167, 60, 69, 203, 12, 180, 198, 9, 217, 117, 38],
            vector[171, 2, 39, 3, 139, 105, 166, 171, 153, 151, 102, 197, 151, 186, 140, 116, 114, 90, 213, 225, 20, 167, 60, 69, 203, 12, 180, 198, 9, 217, 117, 38],
            pop,
            b"name",
            b"description",
            b"image_url",
            b"project_url",
            addr,
            addr,
            addr,
            addr,
            1,
            0,
            ctx
        );
        test_scenario::return_shared(system_state);
    }

    public fun remove_validator_candidate(validator: address, scenario: &mut Scenario) {
        test_scenario::next_tx(scenario, validator);
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
        let ctx = test_scenario::ctx(scenario);

        sui_system::request_remove_validator_candidate(
            &mut system_state,
            ctx
        );
        test_scenario::return_shared(system_state);
    }

    public fun add_validator(validator: address, scenario: &mut Scenario) {
        test_scenario::next_tx(scenario, validator);
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
        let ctx = test_scenario::ctx(scenario);

        sui_system::request_add_validator_for_testing(
            &mut system_state,
            0,
            ctx
        );
        test_scenario::return_shared(system_state);
    }

    public fun remove_validator(validator: address, scenario: &mut Scenario) {
        test_scenario::next_tx(scenario, validator);
        let system_state = test_scenario::take_shared<SuiSystemState>(scenario);

        let ctx = test_scenario::ctx(scenario);

        sui_system::request_remove_validator(
            &mut system_state,
            ctx
        );
        test_scenario::return_shared(system_state);
    }

    public fun assert_validator_self_stake_amounts(validator_addrs: vector<address>, stake_amounts: vector<u64>, scenario: &mut Scenario) {
        let i = 0;
        while (i < vector::length(&validator_addrs)) {
            let validator_addr = *vector::borrow(&validator_addrs, i);
            let amount = *vector::borrow(&stake_amounts, i);

            test_scenario::next_tx(scenario, validator_addr);
            let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
            let stake_plus_rewards = stake_plus_current_rewards_for_validator(validator_addr, &system_state, scenario);
            assert_eq(stake_plus_rewards, amount);
            test_scenario::return_shared(system_state);
            i = i + 1;
        };
    }

    public fun assert_validator_total_stake_amounts(validator_addrs: vector<address>, stake_amounts: vector<u64>, scenario: &mut Scenario) {
        let i = 0;
        while (i < vector::length(&validator_addrs)) {
            let validator_addr = *vector::borrow(&validator_addrs, i);
            let amount = *vector::borrow(&stake_amounts, i);

            test_scenario::next_tx(scenario, validator_addr);
            let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
            assert!(sui_system::validator_stake_amount(&mut system_state, validator_addr) == amount, 0);
            test_scenario::return_shared(system_state);
            i = i + 1;
        };
    }

    public fun assert_validator_non_self_stake_amounts(validator_addrs: vector<address>, stake_amounts: vector<u64>, scenario: &mut Scenario) {
        let i = 0;
        while (i < vector::length(&validator_addrs)) {
            let validator_addr = *vector::borrow(&validator_addrs, i);
            let amount = *vector::borrow(&stake_amounts, i);
            test_scenario::next_tx(scenario, validator_addr);
            let system_state = test_scenario::take_shared<SuiSystemState>(scenario);
            let non_self_stake_amount = sui_system::validator_stake_amount(&mut system_state, validator_addr) - stake_plus_current_rewards_for_validator(validator_addr, &system_state, scenario);
            assert!(non_self_stake_amount == amount, 0);
            test_scenario::return_shared(system_state);
            i = i + 1;
        };
    }

    /// Return the rewards for the validator at `addr` in terms of SUI.
    public fun stake_plus_current_rewards_for_validator(addr: address, system_state: &SuiSystemState, scenario: &mut Scenario): u64 {
        let validator_ref = validator_set::get_active_validator_ref(sui_system::validators(system_state), addr);
        let amount = stake_plus_current_rewards(addr, validator::get_staking_pool_ref(validator_ref), scenario);
        amount
    }

    public fun stake_plus_current_rewards(addr: address, staking_pool: &StakingPool, scenario: &mut Scenario): u64 {
        let sum = 0;
        test_scenario::next_tx(scenario, addr);
        let stake_ids = test_scenario::ids_for_sender<StakedSui>(scenario);
        let current_epoch = tx_context::epoch(test_scenario::ctx(scenario));

        while (!vector::is_empty(&stake_ids)) {
            let staked_sui_id = vector::pop_back(&mut stake_ids);
            let staked_sui = test_scenario::take_from_sender_by_id<StakedSui>(scenario, staked_sui_id);
            sum = sum + staking_pool::calculate_rewards(staking_pool, &staked_sui, current_epoch);
            test_scenario::return_to_sender(scenario, staked_sui);
        };
        sum
    }

    public fun total_sui_balance(addr: address, scenario: &mut Scenario): u64 {
        let sum = 0;
        test_scenario::next_tx(scenario, addr);
        let coin_ids = test_scenario::ids_for_sender<Coin<SUI>>(scenario);
        let i = 0;
        while (i < vector::length(&coin_ids)) {
            let coin = test_scenario::take_from_sender_by_id<Coin<SUI>>(scenario, *vector::borrow(&coin_ids, i));
            sum = sum + coin::value(&coin);
            test_scenario::return_to_sender(scenario, coin);
            i = i + 1;
        };
        sum
    }
}
