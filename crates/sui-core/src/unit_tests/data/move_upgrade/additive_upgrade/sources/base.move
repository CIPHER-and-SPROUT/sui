// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module base_addr::base {

    public struct A<T> {
        f1: bool,
        f2: T
    }

    // new struct is fine
    public struct B<T> {
        f2: bool,
        f1: T,
    }

    /* friend base_addr::friend_module; */

    // new function is fine
    public fun return_1(): u64 { 1 }

    public fun return_0(): u64 { abort 42 }

    public fun plus_1(x: u64): u64 { x + 1 }

    public(package) fun friend_fun(x: u64): u64 { x }

    fun non_public_fun(y: bool): u64 { if (y) 0 else 1 }

    entry fun entry_fun() { }
}
