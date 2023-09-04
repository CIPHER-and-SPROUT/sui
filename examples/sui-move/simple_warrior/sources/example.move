// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Demonstrates wrapping objects using the `Option` type.
module simple_warrior::example {
    use std::option::{Self, Option};
    use sui::object::{Self, UID};
    use sui::tx_context::TxContext;

    struct Sword has key, store {
        id: UID,
        strength: u8,
    }

    struct Warrior has key {
        id: UID,
        sword: Option<Sword>,
    }

    public fun new_sword(strength: u8, ctx: &mut TxContext): Sword {
        Sword { id: object::new(ctx), strength }
    }

    public fun new_warrior(ctx: &mut TxContext): Warrior {
        Warrior { id: object::new(ctx), sword: option::none() }
    }

    public fun equip(warrior: &mut Warrior, sword: Sword): Option<Sword> {
        option::swap_or_fill(&mut warrior.sword, sword)
    }

    // === Tests ===
    use sui::test_scenario as ts;

    #[test]
    fun test_equip_empty() {
        let ts = ts::begin(@0xA);
        let s = new_sword(42, ts::ctx(&mut ts));
        let w = new_warrior(ts::ctx(&mut ts));

        let prev = equip(&mut w, s);
        option::destroy_none(prev);

        let Warrior { id, sword } = w;
        object::delete(id);

        let Sword { id, strength: _ } = option::destroy_some(sword);
        object::delete(id);

        ts::end(ts);
    }

    #[test]
    fun test_equip_swap() {
        let ts = ts::begin(@0xA);
        let s1 = new_sword(21, ts::ctx(&mut ts));
        let s2 = new_sword(42, ts::ctx(&mut ts));
        let w = new_warrior(ts::ctx(&mut ts));

        let prev = equip(&mut w, s1);
        option::destroy_none(prev);

        let prev = equip(&mut w, s2);
        let Sword { id, strength } = option::destroy_some(prev);
        assert!(strength == 21, 0);
        object::delete(id);

        let Warrior { id, sword } = w;
        object::delete(id);

        let Sword { id, strength } = option::destroy_some(sword);
        assert!(strength == 42, 0);
        object::delete(id);

        ts::end(ts);
    }
}
