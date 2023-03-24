// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module sui::royalty_policy_tests {
    use sui::coin;
    use sui::sui::SUI;
    use sui::royalty_policy;
    use sui::tx_context::dummy as ctx;
    use sui::transfer_policy as policy;
    use sui::transfer_policy_tests as test;

    #[test]
    fun test_default_flow() {
        let ctx = &mut ctx();
        let (policy, cap) = test::prepare(ctx);

        // 1% royalty
        royalty_policy::set(&mut policy, &cap, 100);

        let request = policy::new_request(100_000, test::fresh_id(ctx), ctx);
        let payment = coin::mint_for_testing<SUI>(2000, ctx);

        royalty_policy::pay(&mut policy, &mut request, &mut payment, ctx);
        policy::confirm_request(&mut policy, request);

        let remainder = coin::burn_for_testing(payment);
        let profits = test::wrapup(policy, cap, ctx);

        assert!(remainder == 1000, 0);
        assert!(profits == 1000, 1);
    }

    #[test]
    #[expected_failure(abort_code = sui::royalty_policy::EIncorrectArgument)]
    fun test_incorrect_config() {
        let ctx = &mut ctx();
        let (policy, cap) = test::prepare(ctx);

        royalty_policy::set(&mut policy, &cap, 11_000);
        test::wrapup(policy, cap, ctx);
    }

    #[test]
    #[expected_failure(abort_code = sui::royalty_policy::EInsufficientAmount)]
    fun test_insufficient_amount() {
        let ctx = &mut ctx();
        let (policy, cap) = test::prepare(ctx);

        // 1% royalty
        royalty_policy::set(&mut policy, &cap, 100);

        // Requires 1_000 MIST, coin has only 999
        let request = policy::new_request(100_000, test::fresh_id(ctx), ctx);
        let payment = coin::mint_for_testing<SUI>(999, ctx);

        royalty_policy::pay(&mut policy, &mut request, &mut payment, ctx);
        policy::confirm_request(&mut policy, request);

        coin::burn_for_testing(payment);
        test::wrapup(policy, cap, ctx);
    }
}
