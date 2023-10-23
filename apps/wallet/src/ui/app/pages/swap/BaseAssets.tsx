// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useSortedCoinsByCategories } from '_app/hooks/useSortedCoinsByCategories';
import Loading from '_components/loading';
import Overlay from '_components/overlay';
import { filterAndSortTokenBalances } from '_helpers';
import { useActiveAddress, useCoinsReFetchingConfig } from '_hooks';
import { TokenRow } from '_pages/home/tokens/TokensDetails';
import { useSuiClientQuery } from '@mysten/dapp-kit';
import { useNavigate } from 'react-router-dom';

export function BaseAssets() {
	const navigate = useNavigate();
	const selectedAddress = useActiveAddress();
	const { staleTime, refetchInterval } = useCoinsReFetchingConfig();

	const { data: coins, isPending } = useSuiClientQuery(
		'getAllBalances',
		{ owner: selectedAddress! },
		{
			enabled: !!selectedAddress,
			refetchInterval,
			staleTime,
			select: filterAndSortTokenBalances,
		},
	);

	const { recognized } = useSortedCoinsByCategories(coins ?? []);

	return (
		<Overlay showModal title="Select a Coin" closeOverlay={() => navigate(-1)}>
			<Loading loading={isPending}>
				<div className="flex flex-shrink-0 justify-start flex-col w-full">
					{recognized?.map((coinBalance, index) => {
						return (
							<>
								<TokenRow
									key={coinBalance.coinType}
									coinBalance={coinBalance}
									onClick={() => {
										navigate(
											`/swap?${new URLSearchParams({ type: coinBalance.coinType }).toString()}`,
										);
									}}
								/>

								{index !== recognized.length - 1 && <div className="bg-gray-45 h-px w-full" />}
							</>
						);
					})}
				</div>
			</Loading>
		</Overlay>
	);
}
