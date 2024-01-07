// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// A betting game that depends on Sui randomness:
/// 1. Anyone can create a new game by depositing SUIs as the initial balance.
/// 2. Anyone can play the game by betting on X SUIs. They win X with probability 49% and loss the X SUIs otherwise.
///    A user calls start_spin() to play the game. The start_spin() function returns a spin_id that can be used to
///    complete the spin by calling complete_spin().
/// 3. Anyone (including the game owner) can force completion of all spins that are ready to be completed by calling
///    force_complete().
/// 4. Spins that is not completed within the maximal time window of Random can be liquidated.
///
module games::slot_machine {

    use std::vector;
    use sui::balance::{Self, Balance};
    use sui::coin::{Self, Coin};
    use sui::object::{Self, UID};
    use sui::random::{Self, RandomGeneratorRequest, Random};
    use sui::sui::SUI;
    use sui::table::{Self, Table};
    use sui::transfer::{Self, share_object};
    use sui::tx_context::{Self, TxContext};

    const EWrongCaller: u64 = 0;
    const EBetTooLarge: u64 = 1;

    struct Game has key {
        id: UID,
        owner: address,
        balance: Balance<SUI>,

        // The "data structure" of ongoing spins.
        num_of_spins: u64,
        incomplete_spins: vector<Spin>,
        spin_id_to_index: Table<u64, u64>, // used for O(1) lookup of spin index
    }

    struct Spin has store {
        spin_id: u64,
        recepient: address,
        locked_balance: Balance<SUI>,
        randomness_request: RandomGeneratorRequest,
    }

    struct SpinTicket has key {
        id: UID,
        spin_id: u64,
        waiting_for_round: u64,
    }

    /// Anyone can create a new game with an initial balance.
    public fun create(initial_balance: Coin<SUI>, ctx: &mut TxContext) {
        share_object(Game {
            id: object::new(ctx),
            owner: tx_context::sender(ctx),
            balance: coin::into_balance(initial_balance),
            num_of_spins: 0,
            incomplete_spins: vector::empty(),
            spin_id_to_index: table::new(ctx),
        });
    }

    /// The owner can withdraw all the balance from the game (but not of ongoing spins).
    public fun withdraw(game: &mut Game, ctx: &mut TxContext): Coin<SUI> {
        assert!(tx_context::sender(ctx) == game.owner, EWrongCaller);
        let amount = balance::value(&game.balance);
        coin::take(&mut game.balance, amount, ctx)
    }

    /// Start a new spin.
    public fun start_spin(game :&mut Game, bet: Coin<SUI>, r: &Random, ctx: &mut TxContext): SpinTicket {
        assert!(coin::value(&bet) <= balance::value(&game.balance), EBetTooLarge);
        // Lock the total amount of the spin.
        let locked_balance = balance::split(&mut game.balance, coin::value(&bet));
        coin::put(&mut locked_balance, bet);
        let spin_id = game.num_of_spins;
        let spin = Spin {
            spin_id,
            recepient: tx_context::sender(ctx),
            locked_balance,
            randomness_request: random::new_request(r, ctx),
        };
        let ticket = SpinTicket {
            id: object::new(ctx),
            spin_id,
            waiting_for_round: random::required_round(&spin.randomness_request),
        };
        // Update the data structure of ongoing spins.
        game.num_of_spins = game.num_of_spins + 1;
        vector::push_back(&mut game.incomplete_spins, spin);
        table::add(&mut game.spin_id_to_index, spin_id, vector::length(&game.incomplete_spins) - 1);

        ticket
    }

    fun remove(spin_id: u64, game: &mut Game): Spin {
        let i = table::remove(&mut game.spin_id_to_index, spin_id);
        let last = vector::length(&game.incomplete_spins) - 1;
        vector::swap(&mut game.incomplete_spins, i, last);
        let spin = vector::pop_back(&mut game.incomplete_spins);
        // Update the map to the swapped spin if it wasn't the last spin.
        if (i < vector::length(&game.incomplete_spins)) {
            let swapped_spin_id = vector::borrow(&game.incomplete_spins, i).spin_id;
            *table::borrow_mut(&mut game.spin_id_to_index, swapped_spin_id) = i;
        };
        spin
    }

    fun process(spin: Spin, game: &mut Game, r: &Random, ctx: &mut TxContext) {
        let Spin { spin_id: _ , recepient, locked_balance, randomness_request } = spin;
        let gen = random::fulfill(&randomness_request, r);
        let random_number = random::generate_u8_in_range(&mut gen, 1, 100);
        let win = random_number < 50; // 49% chance of winning
        if (win) {
            let coin_to_send = coin::from_balance(locked_balance, ctx);
            transfer::public_transfer(coin_to_send, recepient);
            // TODO: emit event?
        } else {
            balance::join(&mut game.balance, locked_balance);
            // TODO: emit event?
        };
    }

    fun liquidate(spin: Spin, game: &mut Game) {
        let Spin { spin_id: _, recepient: _, locked_balance, randomness_request: _ } = spin;
        balance::join(&mut game.balance, locked_balance);
        // TODO: emit event?
    }

    /// Complete a spin (can be called by anyone).
    public fun complete_spin(spin_ticket: SpinTicket, game: &mut Game, r: &Random, ctx: &mut TxContext) {
        let SpinTicket { id, spin_id, waiting_for_round: _ } = spin_ticket;
        object::delete(id);
        let spin = remove(spin_id, game);
        process(spin, game, r, ctx);
    }

    /// Complete *all* ongoing spins that are ready to be completed, and liquidate old ones if needed.
    public fun force_complete(game: &mut Game, r: &Random, ctx: &mut TxContext) {
        let i = 0;
        while (i < vector::length(&game.incomplete_spins)) {
            let spin = vector::borrow(&game.incomplete_spins, i);
            if (random::is_available(&spin.randomness_request, r)) {
                let spin = remove(spin.spin_id, game);
                process(spin, game, r, ctx);
                continue // not incrementing i
            };
            if (random::is_too_old(&spin.randomness_request, r)) {
                let spin = remove(spin.spin_id, game);
                liquidate(spin, game);
                continue // not incrementing i
            };
            i = i + 1;
        };
    }

    public fun get_balance(game: &Game): u64 {
        balance::value(&game.balance)
    }
}
