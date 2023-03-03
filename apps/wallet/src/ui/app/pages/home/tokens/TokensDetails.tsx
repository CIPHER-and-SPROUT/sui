// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SUI_TYPE_ARG, Coin } from '@mysten/sui.js';
import { useMemo } from 'react';

import { CoinActivitiesCard } from './CoinActivityCard';
import { TokenIconLink } from './TokenIconLink';
import CoinBalance from './coin-balance';
import IconLink from './icon-link';
import { useActiveAddress } from '_app/hooks/useActiveAddress';
import { Text } from '_app/shared/text';
import Alert from '_components/alert';
import { SuiIcons } from '_font-icons/output/sui-icons';
import { useAppSelector, useGetAllBalances, useGetCoinBalance } from '_hooks';
import { AccountSelector } from '_src/ui/app/components/AccountSelector';
import PageTitle from '_src/ui/app/shared/PageTitle';
import FaucetRequestButton from '_src/ui/app/shared/faucet/FaucetRequestButton';

type TokenDetailsProps = {
    coinType?: string;
};

function MyTokens() {
    const accountAddress = useActiveAddress();
    const { data: balance } = useGetAllBalances(accountAddress);

    const noSuiToken = !balance?.find(
        ({ coinType }) => coinType === SUI_TYPE_ARG
    );

    return (
        <>
            {balance?.length ? (
                <div className="flex flex-1 justify-start flex-col w-full mt-6">
                    <Text variant="caption" color="steel" weight="semibold">
                        MY COINS
                    </Text>
                    <div className="flex flex-col w-full justify-center divide-y divide-solid divide-steel/20 divide-x-0">
                        {balance.map(({ coinType, totalBalance }) => (
                            <CoinBalance
                                type={coinType}
                                balance={totalBalance}
                                key={coinType}
                            />
                        ))}
                    </div>
                </div>
            ) : null}
            {noSuiToken ? (
                <div className="flex flex-col flex-nowrap justify-center items-center gap-2 text-center mt-6 px-2.5">
                    <FaucetRequestButton trackEventSource="home" />
                    <Text variant="p2" color="gray-80" weight="normal">
                        To conduct transactions on the Sui network, you need SUI
                        in your wallet.
                    </Text>
                </div>
            ) : null}
        </>
    );
}

function TokenDetails({ coinType }: TokenDetailsProps) {
    const activeCoinType = coinType || SUI_TYPE_ARG;
    const accountAddress = useAppSelector(({ account }) => account.address);
    const {
        data: coinBalance,
        isError,
        errorUpdateCount,
    } = useGetCoinBalance(activeCoinType, accountAddress);

    const tokenBalance = coinBalance?.totalBalance || BigInt(0);

    const coinSymbol = useMemo(
        () => Coin.getCoinSymbol(activeCoinType),
        [activeCoinType]
    );

    return (
        <>
            {coinType && <PageTitle title={coinSymbol} back="/tokens" />}

            <div
                className="flex flex-col h-full flex-1 flex-grow items-center"
                data-testid="coin-page"
            >
                {!coinType && <AccountSelector />}
                <div className="mt-1.5">
                    <CoinBalance
                        balance={tokenBalance}
                        type={activeCoinType}
                        mode="standalone"
                    />
                </div>
                {isError || errorUpdateCount > 0 ? (
                    <Alert>
                        <div>
                            <strong>Error updating balance</strong>
                        </div>
                    </Alert>
                ) : null}
                <div className="flex flex-nowrap gap-2 justify-center w-full mt-5">
                    <IconLink
                        icon={SuiIcons.Buy}
                        to="/"
                        disabled={true}
                        text="Buy"
                    />
                    <IconLink
                        icon={SuiIcons.ArrowLeft}
                        to={`/send${
                            coinBalance?.coinType
                                ? `?${new URLSearchParams({
                                      type: coinBalance.coinType,
                                  }).toString()}`
                                : ''
                        }`}
                        disabled={!tokenBalance}
                        text="Send"
                    />
                    <IconLink
                        icon={SuiIcons.Swap}
                        to="/"
                        disabled={true}
                        text="Swap"
                    />
                </div>

                {activeCoinType === SUI_TYPE_ARG && accountAddress ? (
                    <div className="mt-6 flex justify-start gap-2 flex-col w-full">
                        <Text
                            variant="caption"
                            color="steel-darker"
                            weight="semibold"
                        >
                            SUI Stake
                        </Text>
                        <TokenIconLink accountAddress={accountAddress} />
                    </div>
                ) : null}

                {!coinType ? (
                    <MyTokens />
                ) : (
                    <div className="mt-6 flex-1 justify-start gap-2 flex-col w-full">
                        <Text variant="caption" color="steel" weight="semibold">
                            {coinSymbol} activity
                        </Text>
                        <div className="flex flex-col flex-nowrap flex-1">
                            <CoinActivitiesCard coinType={activeCoinType} />
                        </div>
                    </div>
                )}
            </div>
        </>
    );
}

export default TokenDetails;
